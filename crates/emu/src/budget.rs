//! What a guest may spend.

use crate::kernel::SyscallError;
use brisc_hw::cpu::Cpu;

/// What a guest may spend, and what it has spent, in the units of the budget of
/// [`Cpu::run`](brisc_hw::cpu::Cpu::run).
///
/// The hart spends it on instructions, by their [`Weights`](brisc_hw::cpu::Weights), and on what
/// the [`Memory`](brisc_hw::memory::Memory) charges for accesses. A kernel is given the budget with
/// every system call, to spend it on the work of the call. The units mean whatever the weights and
/// the prices say they do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    remaining: u64,
    spent: u64,
}

impl Budget {
    /// The most that a budget holds: [`Cpu::MAX_BUDGET`](brisc_hw::cpu::Cpu::MAX_BUDGET), which is
    /// more than any guest spends in practice. A budget that would hold more holds this.
    pub const MAX: u64 = Cpu::MAX_BUDGET;

    /// A budget of `units`, or [`Budget::MAX`] if that is less.
    pub const fn new(units: u64) -> Self {
        Self { remaining: if units < Self::MAX { units } else { Self::MAX }, spent: 0 }
    }

    /// A budget of [`Budget::MAX`], which no realistic guest spends.
    pub const fn unlimited() -> Self {
        Self::new(Self::MAX)
    }

    /// What is left.
    pub const fn remaining(&self) -> u64 {
        self.remaining
    }

    /// What has been spent so far, by the hart and by the kernel, since the budget was made.
    /// Granting more does not change it.
    pub const fn spent(&self) -> u64 {
        self.spent
    }

    /// Spends `units`, or fails with [`SyscallError::Budget`] and spends nothing if
    /// fewer than that remain.
    ///
    /// A kernel charges before it does the work that it charges for, so that a system call that
    /// the budget does not cover has no effect.
    pub const fn charge<K, M>(&mut self, units: u64) -> Result<(), SyscallError<K, M>> {
        match self.remaining.checked_sub(units) {
            Some(remaining) => {
                self.remaining = remaining;
                self.spent = self.spent.saturating_add(units);
                Ok(())
            }
            None => Err(SyscallError::Budget),
        }
    }

    /// Subtracts what the hart spent, which never exceeds what remained.
    pub(crate) const fn spend(&mut self, units: u64) {
        self.remaining = self.remaining.saturating_sub(units);
        self.spent = self.spent.saturating_add(units);
    }

    /// Adds `units` to what remains, up to [`Budget::MAX`].
    pub const fn grant(&mut self, units: u64) {
        let remaining = self.remaining.saturating_add(units);
        self.remaining = if remaining < Self::MAX { remaining } else { Self::MAX };
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_charge_that_does_not_fit_changes_nothing() {
        let mut budget = Budget::new(10);

        assert_eq!(budget.charge::<(), ()>(4), Ok(()));
        assert_eq!(budget.charge::<(), ()>(7), Err(SyscallError::Budget));
        assert_eq!((budget.remaining(), budget.spent()), (6, 4));

        // Granting adds to what remains, and does not change what has been spent.
        budget.grant(5);
        assert_eq!((budget.remaining(), budget.spent()), (11, 4));
    }

    #[test]
    fn a_budget_holds_at_most_the_maximum() {
        assert_eq!(Budget::new(u64::MAX).remaining(), Budget::MAX);
        assert_eq!(Budget::unlimited().remaining(), Budget::MAX);

        let mut budget = Budget::new(Budget::MAX - 1);
        budget.grant(u64::MAX);
        assert_eq!(budget.remaining(), Budget::MAX);
    }
}
