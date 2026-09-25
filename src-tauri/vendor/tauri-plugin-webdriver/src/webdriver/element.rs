use std::collections::HashMap;

use uuid::Uuid;

/// Represents a `WebDriver` element reference
#[derive(Debug, Clone)]
pub struct ElementRef {
    /// `WebDriver` element ID (returned to client)
    pub id: String,
    /// JavaScript variable name holding the element reference
    pub js_ref: String,
}

/// Storage for element references within a session
#[derive(Debug, Default)]
pub struct ElementStore {
    elements: HashMap<String, ElementRef>,
}

impl ElementStore {
    pub fn new() -> Self {
        Self {
            elements: HashMap::new(),
        }
    }

    /// Store a new element and return its reference
    pub fn store(&mut self) -> ElementRef {
        let id = Uuid::new_v4().to_string();
        // Remove hyphens from UUID for valid JS variable name
        let id_no_hyphens = id.replace('-', "");
        let js_ref = format!("__wd_el_{id_no_hyphens}");

        let elem_ref = ElementRef {
            id: id.clone(),
            js_ref,
        };

        self.elements.insert(id, elem_ref.clone());
        elem_ref
    }

    /// Get element by `WebDriver` ID
    pub fn get(&self, id: &str) -> Option<&ElementRef> {
        self.elements.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_element() {
        let mut store = ElementStore::new();
        let elem = store.store();

        assert!(!elem.id.is_empty());
        assert!(elem.js_ref.starts_with("__wd_el_"));
        // js_ref uses ID without hyphens
        assert!(elem.js_ref.contains(&elem.id.replace('-', "")));
    }

    #[test]
    fn test_get_element() {
        let mut store = ElementStore::new();
        let elem = store.store();
        let id = elem.id.clone();

        let retrieved = store.get(&id).expect("element should exist");
        assert_eq!(retrieved.id, id);
    }

    #[test]
    fn test_js_ref_uses_id_without_hyphens() {
        let mut store = ElementStore::new();
        let elem1 = store.store();
        let elem2 = store.store();

        // js_ref should use ID with hyphens removed for valid JS variable name
        assert_eq!(
            elem1.js_ref,
            format!("__wd_el_{}", elem1.id.replace('-', ""))
        );
        assert_eq!(
            elem2.js_ref,
            format!("__wd_el_{}", elem2.id.replace('-', ""))
        );
    }
}
