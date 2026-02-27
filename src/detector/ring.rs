/// Fixed-capacity ring buffer for f64 values.
pub struct RingFloat {
    data: Vec<f64>,
    pos: usize,
    full: bool,
}

impl RingFloat {
    pub fn new(cap: usize) -> Self {
        Self {
            data: vec![0.0; cap],
            pos: 0,
            full: false,
        }
    }

    pub fn push(&mut self, v: f64) {
        self.data[self.pos] = v;
        self.pos += 1;
        if self.pos >= self.data.len() {
            self.pos = 0;
            self.full = true;
        }
    }

    pub fn len(&self) -> usize {
        if self.full {
            self.data.len()
        } else {
            self.pos
        }
    }

    pub fn slice(&self) -> Vec<f64> {
        let n = self.len();
        let mut out = Vec::with_capacity(n);
        if self.full {
            out.extend_from_slice(&self.data[self.pos..]);
            out.extend_from_slice(&self.data[..self.pos]);
        } else {
            out.extend_from_slice(&self.data[..self.pos]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_push_and_len() {
        let mut r = RingFloat::new(5);
        assert_eq!(r.len(), 0);
        r.push(1.0);
        r.push(2.0);
        r.push(3.0);
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn test_ring_slice_not_full() {
        let mut r = RingFloat::new(5);
        r.push(1.0);
        r.push(2.0);
        r.push(3.0);
        assert_eq!(r.slice(), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_ring_wraps_around() {
        let mut r = RingFloat::new(3);
        r.push(1.0);
        r.push(2.0);
        r.push(3.0);
        assert_eq!(r.len(), 3);
        r.push(4.0);
        assert_eq!(r.len(), 3);
        // Should return in insertion order: 2.0, 3.0, 4.0
        assert_eq!(r.slice(), vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_ring_full_cycle() {
        let mut r = RingFloat::new(3);
        for i in 0..10 {
            r.push(i as f64);
        }
        assert_eq!(r.len(), 3);
        assert_eq!(r.slice(), vec![7.0, 8.0, 9.0]);
    }
}
