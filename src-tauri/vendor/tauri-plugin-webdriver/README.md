# tauri-plugin-webdriver

[![Crates.io](https://img.shields.io/crates/v/tauri-plugin-webdriver.svg)](https://crates.io/crates/tauri-plugin-webdriver)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A W3C WebDriver implementation for Tauri applications, enabling automated testing with standard WebDriver clients like Selenium, WebdriverIO, and Playwright.

## Features

- **Full W3C WebDriver compliance** - 47 endpoints implementing the W3C WebDriver specification
- **Native platform integration** - Uses native WebView APIs for reliable automation
- **Zero configuration** - Just add the plugin and start testing
- **Standard tooling support** - Works with Selenium, WebdriverIO, Playwright, and any W3C-compliant client

### Supported Platforms

| Platform | Status | Backend |
|----------|--------|---------|
| macOS | Full support | WKWebView native APIs |
| Windows | Full support | WebView2 native APIs |
| Linux | Full support | WebKitGTK native APIs |
| Android | Full support | Android WebView native APIs |
| iOS | Full support | WKWebView native APIs |

## Installation

> **Warning**: This plugin exposes automation capabilities via HTTP. Never include it in production builds.

Add to your `Cargo.toml` as a dev/test-only dependency:

```toml
[target.'cfg(debug_assertions)'.dependencies]
tauri-plugin-webdriver = "0.2"
```

Or use a feature flag for more control:

```toml
[features]
webdriver = ["tauri-plugin-webdriver"]

[dependencies]
tauri-plugin-webdriver = { version = "0.1", optional = true }
```

## Usage

### 1. Register the plugin

Use conditional compilation to exclude the plugin from release builds:

```rust
fn main() {
    let builder = tauri::Builder::default();

    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_webdriver::init());

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Or with a feature flag:

```rust
fn main() {
    let builder = tauri::Builder::default();

    #[cfg(feature = "webdriver")]
    let builder = builder.plugin(tauri_plugin_webdriver::init());

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

The WebDriver server starts automatically on `http://127.0.0.1:4445`.

### 2. Connect with a WebDriver client

#### Python (Selenium)

```python
from selenium import webdriver
from selenium.webdriver.common.by import By

# Connect to your Tauri app
driver = webdriver.Remote(
    command_executor="http://127.0.0.1:4445",
    options=webdriver.ChromeOptions()  # Options are accepted but not processed
)

# Interact with your app
driver.get("tauri://localhost")  # Or your app's URL
element = driver.find_element(By.CSS_SELECTOR, "#my-button")
element.click()

# Take a screenshot
driver.save_screenshot("screenshot.png")

driver.quit()
```

#### JavaScript (WebdriverIO)

```javascript
const { remote } = require('webdriverio');

(async () => {
    const browser = await remote({
        hostname: '127.0.0.1',
        port: 4445,
        capabilities: {}
    });

    await browser.url('tauri://localhost');
    const button = await browser.$('#my-button');
    await button.click();

    await browser.deleteSession();
})();
```

#### Rust

```rust
use fantoccini::{ClientBuilder, Locator};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ClientBuilder::native()
        .connect("http://127.0.0.1:4445")
        .await?;

    client.goto("tauri://localhost").await?;

    let button = client.find(Locator::Css("#my-button")).await?;
    button.click().await?;

    client.close().await?;
    Ok(())
}
```

## W3C WebDriver Endpoints

### Session Management
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/status` | Server status |
| POST | `/session` | Create session |
| DELETE | `/session/{id}` | Delete session |

### Timeouts
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/session/{id}/timeouts` | Get timeouts |
| POST | `/session/{id}/timeouts` | Set timeouts |

### Navigation
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/session/{id}/url` | Get current URL |
| POST | `/session/{id}/url` | Navigate to URL |
| GET | `/session/{id}/title` | Get page title |
| POST | `/session/{id}/back` | Go back |
| POST | `/session/{id}/forward` | Go forward |
| POST | `/session/{id}/refresh` | Refresh page |

### Elements
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/session/{id}/element` | Find element |
| POST | `/session/{id}/elements` | Find elements |
| GET | `/session/{id}/element/active` | Get active element |
| POST | `/session/{id}/element/{eid}/element` | Find from element |
| POST | `/session/{id}/element/{eid}/elements` | Find all from element |
| POST | `/session/{id}/element/{eid}/click` | Click element |
| POST | `/session/{id}/element/{eid}/clear` | Clear element |
| POST | `/session/{id}/element/{eid}/value` | Send keys |
| GET | `/session/{id}/element/{eid}/text` | Get text |
| GET | `/session/{id}/element/{eid}/name` | Get tag name |
| GET | `/session/{id}/element/{eid}/attribute/{name}` | Get attribute |
| GET | `/session/{id}/element/{eid}/property/{name}` | Get property |
| GET | `/session/{id}/element/{eid}/css/{prop}` | Get CSS value |
| GET | `/session/{id}/element/{eid}/rect` | Get rect |
| GET | `/session/{id}/element/{eid}/selected` | Is selected |
| GET | `/session/{id}/element/{eid}/displayed` | Is displayed |
| GET | `/session/{id}/element/{eid}/enabled` | Is enabled |
| GET | `/session/{id}/element/{eid}/computedrole` | Get ARIA role |
| GET | `/session/{id}/element/{eid}/computedlabel` | Get ARIA label |
| GET | `/session/{id}/element/{eid}/screenshot` | Element screenshot |

### Shadow DOM
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/session/{id}/element/{eid}/shadow` | Get shadow root |
| POST | `/session/{id}/shadow/{sid}/element` | Find in shadow |
| POST | `/session/{id}/shadow/{sid}/elements` | Find all in shadow |

### Windows
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/session/{id}/window` | Get window handle |
| POST | `/session/{id}/window` | Switch to window |
| DELETE | `/session/{id}/window` | Close window |
| POST | `/session/{id}/window/new` | New window |
| GET | `/session/{id}/window/handles` | Get all handles |
| GET | `/session/{id}/window/rect` | Get window rect |
| POST | `/session/{id}/window/rect` | Set window rect |
| POST | `/session/{id}/window/maximize` | Maximize |
| POST | `/session/{id}/window/minimize` | Minimize |
| POST | `/session/{id}/window/fullscreen` | Fullscreen |

### Frames
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/session/{id}/frame` | Switch to frame |
| POST | `/session/{id}/frame/parent` | Switch to parent |

### Scripts
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/session/{id}/execute/sync` | Execute sync script |
| POST | `/session/{id}/execute/async` | Execute async script |

### Cookies
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/session/{id}/cookie` | Get all cookies |
| POST | `/session/{id}/cookie` | Add cookie |
| DELETE | `/session/{id}/cookie` | Delete all cookies |
| GET | `/session/{id}/cookie/{name}` | Get cookie |
| DELETE | `/session/{id}/cookie/{name}` | Delete cookie |

### Alerts
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/session/{id}/alert/dismiss` | Dismiss alert |
| POST | `/session/{id}/alert/accept` | Accept alert |
| GET | `/session/{id}/alert/text` | Get alert text |
| POST | `/session/{id}/alert/text` | Send alert text |

### Actions
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/session/{id}/actions` | Perform actions |
| DELETE | `/session/{id}/actions` | Release actions |

### Document
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/session/{id}/source` | Get page source |
| GET | `/session/{id}/screenshot` | Take screenshot |
| POST | `/session/{id}/print` | Print to PDF |

## Locator Strategies

The following locator strategies are supported:

| Strategy | Example |
|----------|---------|
| `css selector` | `#id`, `.class`, `div > p` |
| `xpath` | `//div[@id='test']` |
| `tag name` | `button`, `input` |
| `link text` | Exact link text match |
| `partial link text` | Partial link text match |

## Configuration

The WebDriver server runs on port `4445` by default. The server binds to `127.0.0.1` for security.

### Custom Port

You can configure the port in two ways:

**1. Environment variable:**

```bash
TAURI_WEBDRIVER_PORT=9515 cargo tauri dev
```

**2. Programmatically:**

```rust
fn main() {
    let builder = tauri::Builder::default();

    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_webdriver::init_with_port(9515));

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

The port resolution order is:
1. `init_with_port(port)` - uses the specified port (ignores env var)
2. `init()` - checks `TAURI_WEBDRIVER_PORT` env var, falls back to 4445

## Development

```bash
# Build the plugin
cargo build

# Run clippy
cargo clippy --all-targets -- -D warnings -D clippy::pedantic

# Run the example app
cd examples/tauri-app
cargo tauri dev
```

## License

MIT
