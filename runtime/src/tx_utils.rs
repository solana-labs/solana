use hashbrown::HashMap;
use std::ops::Index;
use rand::distributions::{Uniform, Distribution};

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

/// generate_random_shuffle generates array of unique 
/// random numbers in range of [0, vec_size)
pub fn generate_random_shuffle(vec_size: usize) -> Vec<usize> {
    if vec_size <= 0 {
        return Vec::new();
    }

    let mut rng = rand::thread_rng();
    let uniform_distribution = Uniform::new(0usize, vec_size);
    
    let mut v : Vec<usize> = Vec::new();
    let mut current_element = 0;
    let mut hashmap : HashMap<usize, bool> = HashMap::with_capacity(vec_size);

    v.resize(vec_size, 0);
    
    loop {
        if current_element == vec_size {
            break
        }

        let random_element = uniform_distribution.sample(&mut rng);

        if let Some(_) = hashmap.get(&random_element) {
            continue;
        }

        v[current_element] = random_element;
        hashmap.insert(v[current_element], true);
        current_element += 1;
    }

    v
}
