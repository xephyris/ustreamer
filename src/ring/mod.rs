pub struct RingBuffer {
    size: usize,
    bufs: Vec<Vec<u8>>,
    write: usize,
    read: usize,
}

impl RingBuffer {
    pub fn new(size: usize) -> Self {
        RingBuffer {
            size,
            bufs: vec![Vec::new(); size + 1],
            write: 0,
            read: 0,
        }
    }

    pub fn from_vec(vec: Vec<Vec<u8>>) -> Self {
        RingBuffer {
            size: vec.len(),
            bufs: vec,
            write: 0,
            read: 0,
        }
    }

    pub fn write(&mut self, img_buf: Vec<u8>) -> Result<(), RingBufError> {
        if !self.full() {
            self.bufs[self.write] = img_buf;
            self.write = (self.write + 1) % (self.size + 1);
            Ok(())
        } else {
            Err(RingBufError::BufferFull)
        }
    }

    pub fn read(&mut self) -> Result<Vec<u8>, RingBufError> {
        if self.read != self.write {
            let mut output = Vec::new();
            std::mem::swap(&mut output, &mut self.bufs[self.read]);
            self.read = (self.read + 1) % (self.size + 1);
            Ok(output)
        } else {
            Err(RingBufError::DataNotFound)
        }
    }

    pub fn read_write(&mut self, mut img_buf: Vec<u8>) -> Result<Vec<u8>, RingBufError> {
        let mut output = Vec::new();
        std::mem::swap(&mut output, &mut self.bufs[self.read]);
        std::mem::swap(&mut img_buf, &mut self.bufs[self.write]);
        self.read = (self.read + 1) % (self.size + 1);
        self.write = (self.write + 1) % (self.size + 1);
        Ok(output)
    }

    pub fn full(&self) -> bool {
        if (self.write + 1) % (self.size + 1) == self.read {
            true
        } else {
            false
        }
    }

    pub fn remaining_capacity(&self) -> usize {
        if self.read <= self.write {
            self.size - (self.write - self.read)
        } else {
            self.read - self.write
        }
    }

    pub fn slots(&self) -> Vec<bool> {
        // println!("Write Head {} Read Head {}", self.write, self.read);
        let mut filled = vec![false; self.size];
        if self.read != self.write {
            if self.full() {
                filled = vec![true; self.size];
            } else {
                if self.read < self.write {
                    for i in self.read..self.write {
                        filled[i] = true;
                    }
                } else {
                    for i in 0..self.write {
                        filled[i] = true;
                    }
                    for i in self.read..self.size {
                        filled[i] = true;
                    }
                }

                // Wrap Fix for the N + 1 slot
                if !self.bufs[self.size].is_empty() {
                    filled[0] = true;
                }
            }
        }
        filled
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum RingBufError {
    BufferFull,
    DataNotFound,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn buffer_fill_and_release() {
        let mut ring_buf = RingBuffer::new(4);
        for i in 0..4 {
            ring_buf.write(vec![i; 4]).unwrap();
            dbg!(&ring_buf.bufs);
        }
        
        
        assert_eq!(ring_buf.write(vec![4; 4]), Err(RingBufError::BufferFull));
        assert_eq!(vec![true; 4], ring_buf.slots());


        assert_eq!(Ok(vec![0_u8; 4]), ring_buf.read());
        assert_eq!(Ok(vec![1_u8; 4]), ring_buf.read());

        assert_eq!(vec![false, false, true, true], ring_buf.slots());
        dbg!(&ring_buf.bufs);
        assert_eq!(ring_buf.write(vec![4; 4]), Ok(()));
        assert_eq!(vec![true, false, true, true], ring_buf.slots());
        dbg!(&ring_buf.bufs);
        assert_eq!(Ok(vec![2_u8; 4]), ring_buf.read());
        dbg!(&ring_buf.bufs);
        assert_eq!(Ok(vec![3_u8; 4]), ring_buf.read());
        assert_eq!(Ok(vec![4_u8; 4]), ring_buf.read());
        dbg!(&ring_buf.bufs);
        assert_eq!(ring_buf.write(vec![5; 4]), Ok(()));
        assert_eq!(ring_buf.write(vec![6; 4]), Ok(()));
        assert_eq!(ring_buf.write(vec![7; 4]), Ok(()));
        assert_eq!(ring_buf.write(vec![8; 4]), Ok(()));
        assert_eq!(ring_buf.write(vec![9; 4]), Err(RingBufError::BufferFull));
        dbg!(&ring_buf.bufs);

    }
}