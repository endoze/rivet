//! A bounded cache for package icons. Without one, gpui keeps every image it
//! has loaded until the app quits, so scrolling a large package list would
//! hold every icon passed.

use gpui_kit::{App, ImageCache, ImageCacheError, ImageCacheItem, RenderImage, Resource, Window};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Icons kept loaded: several screens of rows, at about 256 KB each.
const CAPACITY: usize = 128;

/// Keeps the most recently drawn icons and frees the rest.
pub struct IconCache {
  items: HashMap<Resource, ImageCacheItem>,
  recent: Recent<Resource>,
}

impl IconCache {
  pub fn new() -> Self {
    Self {
      items: HashMap::new(),
      recent: Recent::new(CAPACITY),
    }
  }
}

impl Default for IconCache {
  fn default() -> Self {
    Self::new()
  }
}

impl ImageCache for IconCache {
  fn load(
    &mut self,
    resource: &Resource,
    window: &mut Window,
    cx: &mut App,
  ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
    if let Some(evicted) = self.recent.touch(resource.clone())
      && let Some(item) = self.items.remove(&evicted)
      && let Some(Ok(image)) = item.get()
    {
      cx.drop_image(image, Some(window));
    }

    self
      .items
      .entry(resource.clone())
      .or_insert_with(|| ImageCacheItem::new(resource, cx))
      .use_image(window)
  }
}

/// Keys in order of last use, holding at most `capacity`.
struct Recent<K> {
  order: VecDeque<K>,
  capacity: usize,
}

impl<K: Eq> Recent<K> {
  fn new(capacity: usize) -> Self {
    Self {
      order: VecDeque::with_capacity(capacity + 1),
      capacity,
    }
  }

  /// Marks `key` as just used, and returns the least recently used key if
  /// that pushed it out.
  fn touch(&mut self, key: K) -> Option<K> {
    if let Some(position) = self.order.iter().position(|used| *used == key) {
      self.order.remove(position);
    }

    self.order.push_back(key);

    if self.order.len() <= self.capacity {
      return None;
    }

    self.order.pop_front()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_least_recently_drawn_icon_is_dropped_first() {
    let mut recent = Recent::new(2);

    assert_eq!(recent.touch("a"), None);
    assert_eq!(recent.touch("b"), None);
    assert_eq!(recent.touch("a"), None);
    assert_eq!(recent.touch("c"), Some("b"));
    assert_eq!(recent.touch("d"), Some("a"));
  }
}
