pub struct FilteredList<T> {
    items: Vec<T>,
    visible: Vec<usize>,
}

impl<T> FilteredList<T> {
    pub fn new(items: Vec<T>) -> Self {
        let visible = (0..items.len()).collect();
        Self { items, visible }
    }

    pub fn set_items(&mut self, items: Vec<T>) {
        self.visible = (0..items.len()).collect();
        self.items = items;
    }

    pub fn refilter<F: FnMut(&T) -> bool>(&mut self, mut pred: F) {
        self.visible = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, t)| pred(t).then_some(i))
            .collect();
    }

    pub fn show_all(&mut self) {
        self.visible = (0..self.items.len()).collect();
    }

    pub fn len(&self) -> usize {
        self.visible.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.visible.iter().filter_map(|&i| self.items.get(i))
    }

    pub fn get(&self, visible_idx: usize) -> Option<&T> {
        self.visible
            .get(visible_idx)
            .and_then(|&i| self.items.get(i))
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn position<F: FnMut(&T) -> bool>(&self, mut pred: F) -> Option<usize> {
        self.visible
            .iter()
            .position(|&i| self.items.get(i).is_some_and(&mut pred))
    }
}

impl<T> Default for FilteredList<T> {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list_is_empty() {
        let list: FilteredList<i32> = FilteredList::default();
        assert_eq!(list.len(), 0);
        assert!(list.is_empty());
        assert_eq!(list.get(0), None);
    }

    #[test]
    fn new_makes_all_visible() {
        let list = FilteredList::new(vec![1, 2, 3]);
        assert_eq!(list.len(), 3);
        assert_eq!(list.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn refilter_restricts_visible() {
        let mut list = FilteredList::new(vec![1, 2, 3, 4, 5]);
        list.refilter(|n| n % 2 == 0);
        assert_eq!(list.len(), 2);
        assert_eq!(list.iter().copied().collect::<Vec<_>>(), vec![2, 4]);
    }

    #[test]
    fn show_all_restores_visibility() {
        let mut list = FilteredList::new(vec![1, 2, 3]);
        list.refilter(|n| *n == 2);
        list.show_all();
        assert_eq!(list.len(), 3);
    }
}
