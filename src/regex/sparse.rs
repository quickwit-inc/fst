pub struct SparseSet {
    dense: Vec<usize>,
    sparse: Vec<usize>,
    size: usize,
}

impl SparseSet {
    pub fn new(size: usize) -> SparseSet {
        SparseSet {
            dense: vec![0; size],
            sparse: vec![0; size],
            size: 0,
        }
    }

    pub fn add(&mut self, ip: usize) -> usize {
        let i = self.size;
        self.dense[i] = ip;
        self.sparse[ip] = i;
        self.size += 1;
        i
    }

    pub fn contains(&self, ip: usize) -> bool {
        let i = self.sparse[ip];
        i < self.size && self.dense[i] == ip
    }

    pub fn clear(&mut self) {
        self.size = 0;
    }
}
