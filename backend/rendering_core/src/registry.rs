// Port of services/rendering/analysis/profile/registry.py.

use crate::page::PageSnapshot;

pub struct PageProfileCollectorSpec<C, V> {
    pub name: String,
    pub collect: Box<dyn Fn(&PageSnapshot, &C) -> V>,
}

pub struct PageProfileRegistry<C, V> {
    collectors: Vec<PageProfileCollectorSpec<C, V>>,
}

impl<C, V> PageProfileRegistry<C, V> {
    pub fn new() -> Self {
        PageProfileRegistry {
            collectors: Vec::new(),
        }
    }

    pub fn register(mut self, name: &str, collect: impl Fn(&PageSnapshot, &C) -> V + 'static) -> Self {
        self.collectors.push(PageProfileCollectorSpec {
            name: name.to_string(),
            collect: Box::new(collect),
        });
        self
    }

    /// Ordered (name, value) pairs, matching Python's insertion-ordered dict.
    pub fn collect(&self, page: &PageSnapshot, context: &C) -> Vec<(String, V)> {
        self.collectors
            .iter()
            .map(|spec| (spec.name.clone(), (spec.collect)(page, context)))
            .collect()
    }
}

impl<C, V> Default for PageProfileRegistry<C, V> {
    fn default() -> Self {
        Self::new()
    }
}
