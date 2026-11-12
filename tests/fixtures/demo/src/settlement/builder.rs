pub struct BatchBuilder {
    limit: u64,
}

impl BatchBuilder {
    pub fn new(limit: u64) -> Self {
        Self { limit }
    }
}

pub fn reconcile(a: u64, b: u64) -> bool {
    a == b
}
