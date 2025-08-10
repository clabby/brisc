#![no_std]
#![no_main]

use alloy_evm::{
    revm::{
        context::{BlockEnv, CfgEnv, TxEnv},
        database::{CacheDB, InMemoryDB},
        primitives::{hardfork::SpecId, Address, TxKind, U256},
        state::AccountInfo,
    },
    EthEvmFactory, Evm, EvmEnv, EvmFactory,
};
use linked_list_allocator::LockedHeap;
use syscalls::Sysno;

extern crate alloc;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

unsafe fn init_allocator(heap_start_addr: *mut u8, heap_size: usize) {
    ALLOCATOR.lock().init(heap_start_addr, heap_size)
}

#[inline(always)]
fn eth_transfer() {
    let from = Address::left_padding_from(&[0xFE]);
    let to = Address::left_padding_from(&[0xFF]);
    let transfer_amount = U256::from(10);

    let factory = EthEvmFactory::default();
    let mut db = CacheDB::<InMemoryDB>::default();

    // Initialize the sender account with a large balance.
    db.insert_account_info(from, AccountInfo { balance: U256::MAX, ..Default::default() });

    let mut evm = factory.create_evm(
        &mut db,
        EvmEnv { cfg_env: CfgEnv::new_with_spec(SpecId::PRAGUE), block_env: BlockEnv::default() },
    );
    let tx_env = TxEnv {
        caller: from,
        kind: TxKind::Call(to),
        value: transfer_amount,
        ..Default::default()
    };

    // Transact and assert the transfer's success.
    let result = evm.transact(tx_env).unwrap();
    assert_eq!(result.state.get(&from).unwrap().info.balance, U256::MAX - transfer_amount);
    assert_eq!(result.state.get(&to).unwrap().info.balance, transfer_amount);
}

#[no_mangle]
pub extern "C" fn _start() {
    // Initialize a 1MB heap.
    static mut HEAP: [u8; 1_000_000] = [0u8; 1_000_000];
    unsafe { init_allocator(HEAP.as_mut_ptr(), HEAP.len()) }

    eth_transfer();

    unsafe {
        let _ = syscalls::syscall1(Sysno::exit, 0);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    unsafe {
        let msg = alloc::format!("Panic: {}", info);
        let _ = syscalls::syscall3(Sysno::write, 2, msg.as_ptr() as usize, msg.len());

        let _ = syscalls::syscall1(Sysno::exit, 2);
    }
    panic!();
}
