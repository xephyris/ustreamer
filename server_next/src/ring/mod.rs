use bytes::Bytes;

use crate::Image;

pub struct RingBuffer {
    size: usize,
    bufs: Vec<Image>,
    write: usize,
    read: usize,
}

impl RingBuffer {
    pub fn new(size: usize) -> Self {
        RingBuffer {
            size,
            bufs: vec![Image::new(Bytes::new()); size + 1],
            write: 0,
            read: 0,
        }
    }

    pub fn from_vec(vec: Vec<Image>) -> Self {
        RingBuffer {
            size: vec.len(),
            bufs: vec,
            write: 0,
            read: 0,
        }
    }

    pub fn write(&mut self, img_buf: Image) -> Result<(), RingBufError> {
        if !self.full() {
            self.bufs[self.write] = img_buf;
            self.write = (self.write + 1) % (self.size + 1);
            Ok(())
        } else {
            Err(RingBufError::BufferFull)
        }
    }

    pub fn read(&mut self) -> Result<Image, RingBufError> {
        if self.read != self.write {
            let mut output = Image::new(Bytes::new());
            std::mem::swap(&mut output, &mut self.bufs[self.read]);
            self.read = (self.read + 1) % (self.size + 1);
            Ok(output)
        } else {
            Err(RingBufError::DataNotFound)
        }
    }

    pub fn read_write(&mut self, mut img_buf: Image) -> Result<Image, RingBufError> {
        let mut output = Image::new(Bytes::new());
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
                if !self.bufs[self.size].frame.is_empty() {
                    filled[0] = true;
                }
            }
        }
        filled
    }

    pub fn raw_data_vec(&self) -> Vec<usize> {
        let mut lengths = Vec::new();
        for buf in self.bufs.iter() {
            lengths.push(buf.frame.len());
        }
        lengths
    }

    pub fn heads(&self) -> (usize, usize) {
        (self.read, self.write)
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum RingBufError {
    BufferFull,
    DataNotFound,
}
