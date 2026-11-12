pub struct Batch {
    pub items: Vec<u64>,
}

pub fn submit_batch(batch: Batch) -> u64 {
    batch.items.iter().sum()
}
