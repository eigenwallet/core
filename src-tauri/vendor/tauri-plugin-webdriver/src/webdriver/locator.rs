/// Locator strategies for finding elements
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocatorStrategy {
    CssSelector,
    LinkText,
    PartialLinkText,
    TagName,
    XPath,
}

impl LocatorStrategy {
    /// Parse locator strategy from `WebDriver` string
    pub fn from_string(s: &str) -> Option<Self> {
        match s {
            "css selector" => Some(Self::CssSelector),
            "link text" => Some(Self::LinkText),
            "partial link text" => Some(Self::PartialLinkText),
            "tag name" => Some(Self::TagName),
            "xpath" => Some(Self::XPath),
            _ => None,
        }
    }

    /// Generate JavaScript expression to find element (just the selector, no wrapper)
    pub fn to_selector_js(self, value: &str) -> String {
        let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");

        match self {
            LocatorStrategy::CssSelector => {
                format!("document.querySelector('{escaped}')")
            }
            LocatorStrategy::TagName => {
                format!("document.getElementsByTagName('{escaped}')[0] || null")
            }
            LocatorStrategy::XPath => {
                format!(
                    r"(function() {{
                        var result = document.evaluate('{escaped}', document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
                        return result.singleNodeValue;
                    }})()"
                )
            }
            LocatorStrategy::LinkText => {
                format!(
                    r"Array.from(document.querySelectorAll('a')).find(a => a.textContent.trim() === '{escaped}') || null"
                )
            }
            LocatorStrategy::PartialLinkText => {
                format!(
                    r"Array.from(document.querySelectorAll('a')).find(a => a.textContent.includes('{escaped}')) || null"
                )
            }
        }
    }

    /// Generate JavaScript expression to find multiple elements
    pub fn to_selector_js_multiple(self, value: &str) -> String {
        let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");

        match self {
            LocatorStrategy::CssSelector => {
                format!("Array.from(document.querySelectorAll('{escaped}'))")
            }
            LocatorStrategy::TagName => {
                format!("Array.from(document.getElementsByTagName('{escaped}'))")
            }
            LocatorStrategy::XPath => {
                format!(
                    r"(function() {{
                        var result = [];
                        var iter = document.evaluate('{escaped}', document, null, XPathResult.ORDERED_NODE_ITERATOR_TYPE, null);
                        var node;
                        while ((node = iter.iterateNext())) {{
                            result.push(node);
                        }}
                        return result;
                    }})()"
                )
            }
            LocatorStrategy::LinkText => {
                format!(
                    r"Array.from(document.querySelectorAll('a')).filter(a => a.textContent.trim() === '{escaped}')"
                )
            }
            LocatorStrategy::PartialLinkText => {
                format!(
                    r"Array.from(document.querySelectorAll('a')).filter(a => a.textContent.includes('{escaped}'))"
                )
            }
        }
    }

    /// Generate JavaScript expression to find a single element from a parent element
    /// Returns an expression that evaluates to a single element (or null)
    /// Assumes `parent` variable is defined
    pub fn to_selector_js_single_from_element(self, value: &str) -> String {
        let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");

        match self {
            LocatorStrategy::CssSelector => {
                format!("parent.querySelector('{escaped}')")
            }
            LocatorStrategy::TagName => {
                format!("parent.getElementsByTagName('{escaped}')[0] || null")
            }
            LocatorStrategy::XPath => {
                format!(
                    r"(function() {{
                        var result = document.evaluate('{escaped}', parent, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
                        return result.singleNodeValue;
                    }})()"
                )
            }
            LocatorStrategy::LinkText => {
                format!(
                    r"Array.from(parent.querySelectorAll('a')).find(a => a.textContent.trim() === '{escaped}') || null"
                )
            }
            LocatorStrategy::PartialLinkText => {
                format!(
                    r"Array.from(parent.querySelectorAll('a')).find(a => a.textContent.includes('{escaped}')) || null"
                )
            }
        }
    }

    /// Generate JavaScript expression to find multiple elements from a parent element
    /// Returns an expression that evaluates to an array-like collection
    /// Assumes `parent` variable is defined
    pub fn to_selector_js_from_element(self, value: &str) -> String {
        let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");

        match self {
            LocatorStrategy::CssSelector => {
                format!("Array.from(parent.querySelectorAll('{escaped}'))")
            }
            LocatorStrategy::TagName => {
                format!("Array.from(parent.getElementsByTagName('{escaped}'))")
            }
            LocatorStrategy::XPath => {
                format!(
                    r"(function() {{
                        var result = [];
                        var iter = document.evaluate('{escaped}', parent, null, XPathResult.ORDERED_NODE_ITERATOR_TYPE, null);
                        var node;
                        while ((node = iter.iterateNext())) {{
                            result.push(node);
                        }}
                        return result;
                    }})()"
                )
            }
            LocatorStrategy::LinkText => {
                format!(
                    r"Array.from(parent.querySelectorAll('a')).filter(a => a.textContent.trim() === '{escaped}')"
                )
            }
            LocatorStrategy::PartialLinkText => {
                format!(
                    r"Array.from(parent.querySelectorAll('a')).filter(a => a.textContent.includes('{escaped}'))"
                )
            }
        }
    }

    /// Generate JavaScript expression to find a single element from a shadow root
    /// Returns an expression that evaluates to a single element (or null)
    /// Assumes `shadow` variable is defined
    pub fn to_selector_js_single_from_shadow(self, value: &str) -> String {
        let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");

        match self {
            LocatorStrategy::CssSelector | LocatorStrategy::TagName => {
                format!("shadow.querySelector('{escaped}')")
            }
            LocatorStrategy::XPath => {
                // XPath from shadow root context
                format!(
                    r"(function() {{
                        var result = document.evaluate('{escaped}', shadow, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
                        return result.singleNodeValue;
                    }})()"
                )
            }
            LocatorStrategy::LinkText => {
                format!(
                    r"Array.from(shadow.querySelectorAll('a')).find(a => a.textContent.trim() === '{escaped}') || null"
                )
            }
            LocatorStrategy::PartialLinkText => {
                format!(
                    r"Array.from(shadow.querySelectorAll('a')).find(a => a.textContent.includes('{escaped}')) || null"
                )
            }
        }
    }

    /// Generate JavaScript expression to find multiple elements from a shadow root
    /// Returns an expression that evaluates to an array-like collection
    /// Assumes `shadow` variable is defined
    pub fn to_selector_js_from_shadow(self, value: &str) -> String {
        let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");

        match self {
            LocatorStrategy::CssSelector | LocatorStrategy::TagName => {
                format!("Array.from(shadow.querySelectorAll('{escaped}'))")
            }
            LocatorStrategy::XPath => {
                format!(
                    r"(function() {{
                        var result = [];
                        var iter = document.evaluate('{escaped}', shadow, null, XPathResult.ORDERED_NODE_ITERATOR_TYPE, null);
                        var node;
                        while ((node = iter.iterateNext())) {{
                            result.push(node);
                        }}
                        return result;
                    }})()"
                )
            }
            LocatorStrategy::LinkText => {
                format!(
                    r"Array.from(shadow.querySelectorAll('a')).filter(a => a.textContent.trim() === '{escaped}')"
                )
            }
            LocatorStrategy::PartialLinkText => {
                format!(
                    r"Array.from(shadow.querySelectorAll('a')).filter(a => a.textContent.includes('{escaped}'))"
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_strategy() {
        assert_eq!(
            LocatorStrategy::from_string("css selector"),
            Some(LocatorStrategy::CssSelector)
        );
        assert_eq!(
            LocatorStrategy::from_string("xpath"),
            Some(LocatorStrategy::XPath)
        );
        assert_eq!(LocatorStrategy::from_string("unknown"), None);
    }

    #[test]
    fn test_css_selector_js() {
        let strategy = LocatorStrategy::CssSelector;
        let js = strategy.to_selector_js("#my-button");

        assert!(js.contains("querySelector"));
        assert!(js.contains("#my-button"));
    }

    #[test]
    fn test_xpath_js() {
        let strategy = LocatorStrategy::XPath;
        let js = strategy.to_selector_js("//div[@id='test']");

        assert!(js.contains("document.evaluate"));
        // Single quotes are escaped in the output
        assert!(js.contains("//div[@id=\\'test\\']"));
    }

    #[test]
    fn test_escaping() {
        let strategy = LocatorStrategy::CssSelector;
        let js = strategy.to_selector_js("div[data-value='test']");

        assert!(js.contains("div[data-value=\\'test\\']"));
    }
}
