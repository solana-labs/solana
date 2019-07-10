use std::ops::Index;

/// OrderedIterator allows iterating with specific order specified
pub struct OrderedIterator<'a, T: 'a> {
    element_order : Option<&'a [usize]>,
    current: usize,
    vec: &'a [T]
}

impl<'a, T> OrderedIterator<'a, T> {
    pub fn new(vec: &'a [T], element_order: Option<&'a [usize]>) -> OrderedIterator<'a, T> {
       if let Some(custom_order) = element_order {
           assert!(custom_order.len() == vec.len());
       }
        OrderedIterator{
            element_order: element_order,
            current: 0,
            vec: vec
        }
    }
}

impl<'a, T> Iterator for OrderedIterator<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.vec.len() {
            None
        } else {
            let index: usize;
            if let Some(custom_order) = self.element_order {
                index = custom_order[self.current];
            } else {
                index = self.current;
            }
            self.current += 1;
            Some(self.vec.index(index))
        }
    }
}
