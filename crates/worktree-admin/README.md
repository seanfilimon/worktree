# Worktree Admin Panel

A modern web-based admin interface for managing Worktree instances, built with **Yew** (Rust WebAssembly) and **shadcn-inspired CSS variables**.

## 🚀 Features

- **Yew Framework**: Fast, safe, and modern Rust WebAssembly frontend
- **Shadcn Design System**: Beautiful UI with CSS variables for theming
- **Type-Safe**: Fully type-safe Rust code compiled to WASM
- **Inline Styles**: CSS-in-Rust using shadcn CSS variables
- **Client-Side Routing**: Fast navigation with yew-router
- **Responsive Design**: Mobile-first responsive layout
- **Dark Mode Ready**: Theme switching with CSS variables
- **Zero JavaScript**: Pure Rust/WASM application

## 📋 Prerequisites

- **Rust** 1.75+ ([Install](https://rustup.rs/))
- **Trunk** ([Install](https://trunkrs.dev/))
  ```bash
  cargo install trunk
  ```
- **wasm32-unknown-unknown target**
  ```bash
  rustup target add wasm32-unknown-unknown
  ```

## 🛠️ Development

### Quick Start

```bash
# Clone the repository
cd worktree/crates/worktree-admin

# Run development server
trunk serve

# Open browser at http://127.0.0.1:3000
```

### Build for Production

```bash
# Build optimized WASM bundle
trunk build --release

# Output will be in ./dist/
```

### Project Structure

```
worktree-admin/
├── src/
│   ├── lib.rs              # Main app and routing
│   ├── types.rs            # Data types and models
│   ├── utils.rs            # Utilities and inline style helpers
│   ├── components/         # Reusable UI components
│   │   ├── mod.rs
│   │   ├── navbar.rs       # Navigation bar
│   │   ├── footer.rs       # Footer component
│   │   ├── card.rs         # Card container
│   │   ├── badge.rs        # Status badges
│   │   ├── button.rs       # Button component
│   │   ├── stat_card.rs    # Statistics display
│   │   ├── repo_card.rs    # Repository card
│   │   └── loading.rs      # Loading spinner
│   ├── pages/              # Page components
│   │   ├── mod.rs
│   │   ├── dashboard.rs    # Dashboard page
│   │   ├── repositories.rs # Repositories list
│   │   ├── statistics.rs   # Statistics page
│   │   ├── settings.rs     # Settings page
│   │   └── not_found.rs    # 404 page
│   └── services/           # API services
│       └── mod.rs
├── styles/
│   ├── variables.css       # Shadcn CSS variables
│   └── main.css           # Main styles
├── index.html             # HTML entry point
├── Trunk.toml            # Trunk configuration
├── Cargo.toml            # Rust dependencies
└── README.md             # This file
```

## 🎨 Styling with Shadcn CSS Variables

This project uses **shadcn-inspired CSS variables** for consistent theming:

### Using Inline Styles in Yew

```rust
use yew::prelude::*;
use crate::utils::{colors, layout, border, shadow, combine_styles};

#[function_component(MyComponent)]
pub fn my_component() -> Html {
    let card_style = combine_styles(&[
        colors::bg_card(),
        border::rounded(),
        shadow::md(),
        "padding: 1.5rem",
    ]);

    html! {
        <div style={card_style}>
            <h2 style={colors::text_foreground()}>{"Hello, Worktree!"}</h2>
        </div>
    }
}
```

### Available Style Utilities

```rust
// Colors
colors::bg_primary()
colors::bg_secondary()
colors::bg_card()
colors::text_foreground()
colors::text_muted_foreground()

// Layout
layout::flex()
layout::flex_col()
layout::items_center()
layout::justify_between()
layout::gap("1rem")

// Borders
border::rounded()
border::rounded_lg()
border::border()

// Shadows
shadow::sm()
shadow::md()
shadow::lg()

// Typography
typography::text_lg()
typography::font_semibold()
```

### CSS Variables

All colors use HSL format with CSS variables:

```css
:root {
  --background: 0 0% 100%;
  --foreground: 222.2 84% 4.9%;
  --primary: 221.2 83.2% 53.3%;
  --secondary: 210 40% 96.1%;
  --destructive: 0 84.2% 60.2%;
  --success: 142.1 76.2% 36.3%;
  --warning: 45.4 93.4% 47.5%;
  --border: 214.3 31.8% 91.4%;
  --radius: 0.5rem;
}
```

## 🧩 Components

### Navbar

Navigation bar with routing links and theme toggle.

```rust
use yew::prelude::*;
use crate::components::Navbar;

html! { <Navbar /> }
```

### Card

Generic container for content sections.

```rust
use crate::components::Card;

html! {
    <Card title="Server Status">
        <p>{"Server is running"}</p>
    </Card>
}
```

### Badge

Status indicators with color variants.

```rust
use crate::components::{Badge, BadgeVariant};

html! {
    <Badge variant={BadgeVariant::Success}>{"Active"}</Badge>
}
```

### Button

Interactive buttons with variants.

```rust
use crate::components::{Button, ButtonVariant};

html! {
    <Button variant={ButtonVariant::Primary} onclick={callback}>
        {"Click Me"}
    </Button>
}
```

## 📄 Pages

- **Dashboard** (`/`) - Overview with statistics and recent activity
- **Repositories** (`/repositories`) - List and manage repositories
- **Statistics** (`/stats`) - Detailed statistics and charts
- **Settings** (`/settings`) - Application configuration
- **404** - Not found page

## 🔧 Configuration

### Trunk.toml

Trunk build configuration:

```toml
[build]
target = "index.html"
dist = "dist"
public_url = "/"

[serve]
address = "127.0.0.1"
port = 3000
```

### Cargo Features

```toml
[features]
default = []
ssr = ["axum", "tokio"]  # Server-side rendering (future)
```

## 🧪 Testing

```bash
# Run tests
cargo test

# Run WASM tests
wasm-pack test --headless --chrome
```

## 📦 Building

### Development Build

```bash
trunk serve
```

### Production Build

```bash
trunk build --release
```

The optimized bundle will be in `./dist/`:
- `index.html` - Entry point
- `*.wasm` - WebAssembly binary
- `*.js` - JS glue code
- `*.css` - Compiled styles

### Build Size Optimization

The production build is optimized for size:
- LTO enabled
- Opt-level "z" (size optimization)
- Single codegen unit
- Typical bundle: ~100-200KB gzipped

## 🌐 Deployment

### Static Hosting

Deploy the `dist/` folder to any static host:

```bash
# Netlify
netlify deploy --dir=dist --prod

# Vercel
vercel --prod dist

# GitHub Pages
# Push dist/ to gh-pages branch
```

### Nginx

```nginx
server {
    listen 80;
    server_name admin.worktree.dev;
    root /var/www/worktree-admin/dist;
    
    location / {
        try_files $uri $uri/ /index.html;
    }
}
```

## 🎯 Roadmap

- [x] Yew framework setup
- [x] Shadcn CSS variables
- [x] Inline style utilities
- [x] Basic components (Navbar, Card, Badge, Button)
- [x] Routing with yew-router
- [ ] API integration
- [ ] Real-time updates with WebSockets
- [ ] Charts and data visualization
- [ ] Advanced filtering and search
- [ ] User authentication
- [ ] Multi-theme support
- [ ] Internationalization (i18n)

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test`
5. Build: `trunk build --release`
6. Submit a pull request

## 📝 License

MIT License - see LICENSE file for details

## 🔗 Resources

- [Yew Documentation](https://yew.rs/)
- [Trunk Documentation](https://trunkrs.dev/)
- [shadcn/ui](https://ui.shadcn.com/)
- [WebAssembly](https://webassembly.org/)

## 💡 Tips

### Hot Reload

Trunk supports hot reloading. Changes to Rust code or CSS will automatically rebuild and refresh the browser.

### Debug Logging

Enable logging in the browser console:

```rust
use log::info;

info!("Debug message");
```

Set log level:
```rust
wasm_logger::init(wasm_logger::Config::new(log::Level::Debug));
```

### Performance

- Use `memo` hooks for expensive computations
- Implement `PartialEq` for props to prevent unnecessary re-renders
- Use `use_effect_with_deps` for side effects

### Theme Switching

Toggle between light and dark themes:

```javascript
// In browser console or via button
document.documentElement.setAttribute('data-theme', 'dark');
```

## 🐛 Troubleshooting

### WASM file not loading

Check browser console for CORS errors. Ensure your server serves `.wasm` files with correct MIME type:
```
application/wasm
```

### Styles not applying

Ensure CSS is linked in `index.html`:
```html
<link data-trunk rel="css" href="styles/main.css" />
```

### Build fails

Clear Trunk cache:
```bash
rm -rf dist/
trunk clean
trunk build
```

---

**Built with ❤️ and 🦀 Rust**