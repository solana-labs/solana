use std::ops::Index;

/// OrderedIterator allows iterating with specific order specified
pub struct OrderedIterator<'a, T: 'a> {
    element_order: Option<&'a [usize]>,
    current: usize,
    vec: &'a [T],
}

impl<'a, T> OrderedIterator<'a, T> {
    pub fn new(vec: &'a [T], element_order: Option<&'a [usize]>) -> OrderedIterator<'a, T> {
        if let Some(custom_order) = element_order {
            assert!(custom_order.len() == vec.len());
        }
        OrderedIterator {
            element_order,
            current: 0,
            vec,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ordered_iterator_custom_order() {
        let vec: Vec<usize> = vec![1, 2, 3, 4];
        let custom_order: Vec<usize> = vec![3, 1, 0, 2];

        let ordered_iterator = OrderedIterator::new(&vec, Some(&custom_order));
        let expected_response: Vec<usize> = vec![4, 2, 1, 3];

        let resp: Vec<(&usize, &usize)> = ordered_iterator
            .zip(expected_response.iter())
            .filter(|(actual_elem, expected_elem)| *actual_elem == *expected_elem)
            .collect();

        assert_eq!(resp.len(), custom_order.len());
    }

    #[test]
    fn test_ordered_iterator_original_order() {
        let vec: Vec<usize> = vec![1, 2, 3, 4];
        let ordered_iterator = OrderedIterator::new(&vec, None);

        let resp: Vec<(&usize, &usize)> = ordered_iterator
            .zip(vec.iter())
            .filter(|(actual_elem, expected_elem)| *actual_elem == *expected_elem)
            .collect();

        assert_eq!(resp.len(), vec.len());
    }
}
