# Build System for Markdown to LaTeX-style HTML

This build system converts Markdown files to HTML with LaTeX-style formatting
using the existing LaTeX CSS framework. The build system is written in
TypeScript for better type safety and developer experience.

## Files

### Content Files

- `content/*.md` - Source markdown files (automatically discovered)
- `dist/*.html` - Generated HTML outputs (not committed to git)

### Build System Files

- `builder/build.ts` - TypeScript build script that converts markdown to HTML
- `builder/test.ts` - TypeScript test script to verify the build system
- `tsconfig.json` - TypeScript configuration

## Usage

### Build once

```bash
npm run build:md
```

### Build for static deployment (with local assets)

```bash
npm run build:static
```

This downloads all GitHub release assets locally to `dist/assets/` and modifies the download page to reference these local files instead of GitHub URLs. This enables fully static deployment without dependencies on GitHub's servers.

### Watch for changes (development mode)

```bash
npm run watch
```

### Test the build system

```bash
npm run test
```

## Features

- **Automatic file discovery**: Automatically finds and builds all `.md` files
  in the content directory
- **Clean TypeScript implementation**: Pure TypeScript with proper type safety
  and developer experience
- **Organized structure**: Separates content from build tools for better
  maintainability
- **Markdown to HTML conversion**: Uses the `marked` library for parsing
- **Footnote support**: Converts `^[1]` references to proper HTML footnotes
- **Math expression preservation**: Keeps LaTeX math expressions for MathJax
- **LaTeX styling**: Maintains original LaTeX CSS styling and layout
- **References processing**: Converts reference list to proper footnotes section
- **Watch mode**: Automatically rebuilds when files change
- **Comprehensive testing**: Full test suite to verify functionality
- **Abstract extraction**: Properly separates abstract content from main content
- **Error handling**: Robust error handling with proper exit codes
- **Multiple pages**: Supports building multiple markdown files to separate HTML
  pages

## How it works

1. **Markdown parsing**: The `marked` library converts markdown to HTML
2. **Footnote processing**: Post-processes the HTML to convert `^[1]` to proper
   footnote links
3. **Content separation**: Extracts abstract content and removes duplicates from
   main content
4. **Reference section**: Converts the References section to a proper footnotes
   div
5. **HTML template**: Wraps the generated content in the original HTML structure
6. **Asset linking**: Preserves links to LaTeX CSS, MathJax, and other assets

## TypeScript Interfaces

The build system defines several TypeScript interfaces for type safety:

```typescript
interface FootnoteMatch {
  num: string;
  content: string;
}

interface AbstractMatch {
  content: string;
  previousText: string;
  link: string;
  linkText: string;
}
```

## Folder Structure

```
vision/
├── content/
│   ├── index.md             # Main page content
│   ├── eigenweb.md          # Eigenweb page content
│   └── *.md                 # Any additional markdown files
├── builder/
│   ├── build.ts             # Build script
│   └── test.ts              # Test suite
├── dist/                    # Generated outputs (not in git)
│   ├── index.html           # Generated main page
│   ├── eigenweb.html        # Generated eigenweb page
│   └── *.html               # Generated pages from markdown
├── fonts/                   # LaTeX fonts
├── imgs/                    # Images
├── lang/                    # Language CSS
├── prism/                   # Syntax highlighting
├── latex.css               # Main styling
├── package.json            # Dependencies
├── tsconfig.json           # TypeScript config
└── BUILD.md                # Documentation
```

## Customization

To modify the build process:

1. Edit `builder/build.ts` to change the conversion logic
2. Add new `.md` files to the `content/` directory (they will be automatically
   discovered)
3. Update existing markdown files to modify content
4. Run `npm run build:md` to rebuild all pages
5. Run `npm run test` to verify the changes

The build system automatically discovers all `.md` files in the content
directory and generates corresponding HTML files in the dist directory. The
generated HTML maintains the same styling and functionality as the original
design.
