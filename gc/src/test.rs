use super::{Gc, GcCell, Trace, force_collect};

static mut has_been_destroyed: bool = false;
struct ValidChecker {
    valid: bool,
}

impl ValidChecker {
    fn new() -> ValidChecker {
        unsafe { has_been_destroyed = false; }

        ValidChecker { valid: true }
    }

    fn assert_valid(&self) {
        assert!(self.valid, "Invalid valid checker!")
    }
}

impl Drop for ValidChecker {
    fn drop(&mut self) {
        self.valid = false;
        unsafe { has_been_destroyed = true; }
    }
}

impl Trace for ValidChecker {
    fn trace(&self) {}
    fn root(&self) {}
    fn unroot(&self) {}
}

#[test]
fn basic_allocate() {
    {
        let gced_val = Gc::new(ValidChecker::new());

        gced_val.assert_valid();
        force_collect();
        gced_val.assert_valid();
    }

    assert!(unsafe { !has_been_destroyed }, "Shouldn't have been destroyed yet");
    force_collect();
    assert!(unsafe { has_been_destroyed }, "Should have been destroyed");
}
