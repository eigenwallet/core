import fs from 'fs';
import { marked } from 'marked';

interface TestResult {
  name: string;
  passed: boolean;
  message: string;
}

/**
 * Run a test and return the result
 */
function runTest(
  name: string,
  testFn: () => boolean,
  message: string
): TestResult {
  try {
    const passed = testFn();
    return { name, passed, message: passed ? '✓ ' + message : '✗ ' + message };
  } catch (error) {
    return { name, passed: false, message: `✗ ${message} - Error: ${error}` };
  }
}

/**
 * Test basic markdown parsing
 */
function testBasicMarkdown(): TestResult {
  const testMarkdown = `# Test Header

This is a **bold** and *italic* text with ^[1] footnote.

[1]: This is a footnote reference.`;

  const result = marked(testMarkdown);
  const hasHeader = result.includes('<h1>Test Header</h1>');
  const hasBold = result.includes('<strong>bold</strong>');
  const hasItalic = result.includes('<em>italic</em>');

  return runTest(
    'Basic Markdown',
    () => hasHeader && hasBold && hasItalic,
    'Basic markdown parsing (headers, bold, italic)'
  );
}

/**
 * Test math expression preservation
 */
function testMathExpressions(): TestResult {
  const mathTest = `Math expression: $>3$ and $\\leftrightarrow$ should be preserved.`;
  const result = marked(mathTest);
  const hasMath =
    result.includes('$&gt;3$') && result.includes('$\\leftrightarrow$');

  return runTest(
    'Math Expressions',
    () => hasMath,
    'Math expressions are preserved for MathJax'
  );
}

/**
 * Test generated HTML file
 */
function testGeneratedHtml(): TestResult[] {
  const results: TestResult[] = [];

  // Test index file exists
  results.push(
    runTest(
      'Index File Existence',
      () => fs.existsSync('dist/index.html'),
      'Generated index.html file exists in dist folder'
    )
  );

  // Test eigenweb file exists
  results.push(
    runTest(
      'Eigenweb File Existence',
      () => fs.existsSync('dist/eigenweb.html'),
      'Generated eigenweb.html file exists in dist folder'
    )
  );

  if (fs.existsSync('dist/index.html')) {
    const content = fs.readFileSync('dist/index.html', 'utf8');

    // Test for key elements
    results.push(
      runTest(
        'Footnotes',
        () => content.includes('<div class="footnotes">'),
        'Contains footnotes section'
      )
    );

    results.push(
      runTest(
        'MathJax',
        () => content.includes('MathJax'),
        'Contains MathJax configuration'
      )
    );

    results.push(
      runTest(
        'LaTeX CSS',
        () => content.includes('latex.css'),
        'Contains LaTeX CSS link'
      )
    );

    results.push(
      runTest(
        'Abstract Section',
        () => content.includes('<div class="abstract">'),
        'Contains abstract section'
      )
    );

    results.push(
      runTest(
        'Header',
        () => content.includes('<header') && content.includes('eigenwallet'),
        'Contains proper header with logo'
      )
    );

    results.push(
      runTest(
        'No Duplicates',
        () => {
          // Count how many times "eigenwallet</strong>" appears in abstract vs main
          const abstractSection =
            content.match(/<div class="abstract">[\s\S]*?<\/div>/)?.[0] || '';
          const mainSection =
            content.match(/<main>[\s\S]*?<\/main>/)?.[0] || '';
          const abstractCount = (
            abstractSection.match(/eigenwallet<\/em><\/strong>/g) || []
          ).length;
          const duplicateEigenwalletInAbstract = abstractSection.includes(
            '<strong><em>eigenwallet</em></strong> <strong><em>eigenwallet</em></strong>'
          );
          return !duplicateEigenwalletInAbstract;
        },
        'No duplicate eigenwallet text in abstract section'
      )
    );
  }

  return results;
}

/**
 * Main test runner
 */
function main(): void {
  console.log('Running TypeScript Build System Tests\n');

  const allResults: TestResult[] = [];

  // Run individual tests
  allResults.push(testBasicMarkdown());
  allResults.push(testMathExpressions());
  allResults.push(...testGeneratedHtml());

  // Print results
  allResults.forEach(result => {
    console.log(result.message);
  });

  // Summary
  const passed = allResults.filter(r => r.passed).length;
  const total = allResults.length;

  console.log(`\nTest Results: ${passed}/${total} passed`);

  if (passed === total) {
    console.log('✓ All tests passed!');
    process.exit(0);
  } else {
    console.log('✗ Some tests failed');
    process.exit(1);
  }
}

// Run tests if this is the main module
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}

export { main as runTests };
