#!/usr/bin/env node

/**
 * Seed script for Tucano Test API.
 * Populates the system with realistic test cases, test suites, projects,
 * test configurations, test runs (with execution results), attachments,
 * and milestones.
 *
 * Usage:
 *   node scripts/seed-data.mjs [API_BASE_URL]
 */

const candidateUrls = [
  process.argv[2],
  process.env.API_URL,
  process.env.TUCANO_API_URL,
  'http://localhost:3100',
  'http://localhost:8080/api',
  'http://localhost:3000',
].filter(Boolean);

async function resolveBaseUrl() {
  for (const url of candidateUrls) {
    const cleanUrl = url.replace(/\/$/, '');
    try {
      const res = await fetch(`${cleanUrl}/health`, { signal: AbortSignal.timeout(1500) });
      if (res.ok) {
        return cleanUrl;
      }
    } catch {
      // Continue searching
    }
  }
  // Fallback
  return candidateUrls[0]?.replace(/\/$/, '') || 'http://localhost:3100';
}

// 1x1 transparent PNG buffer for mock image attachment
const SAMPLE_PNG_BUFFER = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==',
  'base64'
);

async function runSeed() {
  const baseUrl = await resolveBaseUrl();
  console.log(`\n🌱 Seeding test data to Tucano Test API at: ${baseUrl}\n`);

  async function api(path, options = {}) {
    const res = await fetch(`${baseUrl}${path}`, {
      headers: options.body instanceof FormData ? undefined : { 'content-type': 'application/json' },
      ...options,
    });
    const text = await res.text();
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
    if (!res.ok && res.status !== 409) {
      console.warn(`⚠️  Warning on ${options.method || 'GET'} ${path} (${res.status}):`, data);
    }
    return { status: res.status, ok: res.ok, data };
  }

  // ==========================================
  // 1. TEST CASES
  // ==========================================
  console.log('📌 Creating Test Cases...');
  const testCases = [
    {
      testCaseId: 'TC-AUTH-001',
      title: 'User Login with Valid Credentials',
      description: 'Verify standard email and password authentication on web login portal.',
      preconditions: 'User account exists with verified email in IAM database.',
      steps: [
        { action: 'Navigate to /login page', expectedResult: 'Login form renders with email and password fields' },
        { action: "Enter registered email & password, click 'Sign In'", expectedResult: 'Session token issued, redirected to /dashboard' },
      ],
      expectedResult: 'User successfully authenticated and session cookie is set.',
      priority: 'Critical',
      severity: 'Critical',
      testType: 'Smoke',
      exploratory: false,
      tags: ['auth', 'smoke', 'p0', 'iam'],
    },
    {
      testCaseId: 'TC-AUTH-002',
      title: 'OAuth2 Social Login (Google / GitHub)',
      description: 'Verify federated authentication via third-party OAuth2 providers.',
      preconditions: 'OAuth provider app is configured and callback URL is registered.',
      steps: [
        'Click "Continue with Google" button on login page',
        'Authenticate on Google consent dialog',
        'Verify redirect back to application with authorization code',
      ],
      expectedResult: 'User account linked or created, session established.',
      priority: 'High',
      severity: 'Major',
      testType: 'Functional',
      exploratory: false,
      tags: ['auth', 'oauth', 'sso'],
    },
    {
      testCaseId: 'TC-AUTH-003',
      title: 'Brute Force Rate Limiting on Login Endpoint',
      description: 'Ensure endpoint throttles repeated failed login attempts.',
      preconditions: 'Rate limiter set to maximum 5 failed attempts per minute per IP.',
      steps: [
        { action: 'Send 6 consecutive invalid login requests in 30 seconds', expectedResult: '6th request receives HTTP 429 Too Many Requests' },
      ],
      expectedResult: 'Account lockout active; error message explains retry cooldown.',
      priority: 'Critical',
      severity: 'Critical',
      testType: 'Security',
      exploratory: false,
      tags: ['security', 'rate-limit', 'owasp'],
    },
    {
      testCaseId: 'TC-CART-001',
      title: 'Add Single Item to Shopping Cart',
      description: 'Verify product detail page "Add to Cart" action.',
      preconditions: 'Product is active with inventory count > 0.',
      steps: [
        { action: "Navigate to product PDP and click 'Add to Cart'", expectedResult: 'Cart badge count increments by 1; toast notification displayed' },
      ],
      expectedResult: 'Item appears in cart with correct SKU, price, quantity, and subtotal.',
      priority: 'Critical',
      severity: 'Critical',
      testType: 'Smoke',
      exploratory: false,
      tags: ['cart', 'smoke', 'ecommerce'],
    },
    {
      testCaseId: 'TC-CART-002',
      title: 'Apply Discount Promo Code to Cart',
      description: 'Verify percentage discount calculation on cart subtotal.',
      preconditions: "Active promo code 'SPRING2026' with 15% discount configured.",
      steps: [
        'Open slide-over shopping cart',
        "Type 'SPRING2026' into promo code input",
        "Click 'Apply Promo'",
      ],
      expectedResult: 'Discount line item displayed and grand total recalculated.',
      priority: 'Medium',
      severity: 'Minor',
      testType: 'Functional',
      exploratory: false,
      tags: ['cart', 'coupons', 'pricing'],
    },
    {
      testCaseId: 'TC-CHECKOUT-001',
      title: 'Multi-Step Checkout with Credit Card Payment',
      description: 'Verify full end-to-end checkout flow from address entry to order confirmation.',
      preconditions: 'Cart contains at least one item; valid test card available.',
      steps: [
        { action: 'Enter valid shipping and billing addresses', expectedResult: 'Available shipping rates calculated' },
        { action: 'Select Express Shipping and enter test credit card details', expectedResult: 'Payment processed successfully' },
        { action: 'Submit final order review', expectedResult: 'Order confirmation page with unique order number is displayed' },
      ],
      expectedResult: 'Order created in database, confirmation email dispatched.',
      priority: 'Critical',
      severity: 'Critical',
      testType: 'Functional',
      exploratory: false,
      tags: ['checkout', 'payment', 'p0', 'e2e'],
    },
    {
      testCaseId: 'TC-PAY-001',
      title: '3D Secure 2.0 Cardholder Verification Flow',
      description: 'Verify 3DS challenge authentication on high-risk transactions.',
      preconditions: '3DS enabled payment gateway integration and test challenge card.',
      steps: [
        { action: 'Submit order using 3DS test card', expectedResult: 'Bank 3DS challenge modal iframe renders' },
        { action: "Enter valid OTP '123456'", expectedResult: 'Challenge passes and transaction settles' },
      ],
      expectedResult: 'Transaction status is SETTLED and 3DS liability shift recorded.',
      priority: 'High',
      severity: 'Major',
      testType: 'Regression',
      exploratory: false,
      tags: ['payments', '3ds', 'compliance'],
    },
    {
      testCaseId: 'TC-PAY-002',
      title: 'Refund Processing for Cancelled Order',
      description: 'Verify full refund dispatch through payment gateway adapter.',
      preconditions: 'Order exists in SETTLED state with transaction ID.',
      steps: [
        'Trigger full refund via admin or API endpoint',
        'Verify gateway webhook receipt',
      ],
      expectedResult: 'Refund entry recorded in ledger, customer notified.',
      priority: 'High',
      severity: 'Major',
      testType: 'Functional',
      exploratory: false,
      tags: ['payments', 'refunds', 'gateway'],
    },
    {
      testCaseId: 'TC-SEARCH-001',
      title: 'Faceted Catalog Search and Filtering',
      description: 'Verify full-text query with category, price range, and tag facets.',
      preconditions: 'Search index populated with catalog products.',
      steps: [
        'Search for keyword "leather"',
        'Filter by Category: "Accessories" and Price: "$50-$100"',
        'Sort by "Price: Low to High"',
      ],
      expectedResult: 'Matching products returned matching all active filters.',
      priority: 'Medium',
      severity: 'Minor',
      testType: 'Functional',
      exploratory: false,
      tags: ['search', 'catalog', 'filters'],
    },
    {
      testCaseId: 'TC-SEARCH-002',
      title: 'Unicode and Edge-Case Query Sanitization',
      description: 'Verify search input handling with emoji, symbols, and SQL/XSS strings.',
      preconditions: 'Search engine is running.',
      steps: [
        { action: 'Enter emojis, zero-width characters, and HTML tags in search box', expectedResult: 'Input sanitized cleanly; no 500 error' },
      ],
      expectedResult: 'Empty state with helpful suggestions rendered without error.',
      priority: 'Low',
      severity: 'Trivial',
      testType: 'Exploratory',
      exploratory: true,
      tags: ['search', 'exploratory', 'i18n', 'security'],
    },
    {
      testCaseId: 'TC-INV-001',
      title: 'Real-Time Inventory Stock Depletion on Order Placement',
      description: 'Ensure warehouse inventory reflects purchases in real-time.',
      preconditions: 'SKU-LOG-900 initial stock is 10 units.',
      steps: [
        { action: 'Complete purchase of 2 units of SKU-LOG-900', expectedResult: 'Inventory stock decrements to 8 units in storage' },
      ],
      expectedResult: 'Webhook event dispatched to warehouse logistics ERP within 500ms.',
      priority: 'High',
      severity: 'Critical',
      testType: 'Regression',
      exploratory: false,
      tags: ['inventory', 'real-time', 'erp'],
    },
    {
      testCaseId: 'TC-MOB-001',
      title: 'Mobile Touch Gestures and Responsive Viewport',
      description: 'Verify touch carousel swipe gestures and responsive navigation drawer.',
      preconditions: 'Mobile viewport screen width <= 393px.',
      steps: [
        'Swipe left/right on product image carousel',
        'Pinch to zoom on image modal',
        'Tap hamburger button to open side navigation',
      ],
      expectedResult: 'Smooth touch gesture animations, no horizontal overflow or layout breakage.',
      priority: 'Medium',
      severity: 'Minor',
      testType: 'Exploratory',
      exploratory: true,
      tags: ['mobile', 'touch', 'responsive', 'ui'],
    },
  ];

  for (const tc of testCases) {
    const res = await api('/test_cases', {
      method: 'POST',
      body: JSON.stringify(tc),
    });
    console.log(`  ✓ Test Case: ${tc.testCaseId} - "${tc.title}" (${res.status})`);
  }

  // ==========================================
  // 2. ATTACHMENTS
  // ==========================================
  console.log('\n📎 Uploading Attachments to Test Cases...');
  const attachmentsToUpload = [
    {
      testCaseId: 'TC-AUTH-001',
      filename: 'login-spec.png',
      buffer: SAMPLE_PNG_BUFFER,
      type: 'image/png',
    },
    {
      testCaseId: 'TC-AUTH-003',
      filename: 'rate-limit-response.json',
      buffer: Buffer.from(JSON.stringify({ error: 'rate_limited', retryAfterSeconds: 60, status: 429 }, null, 2)),
      type: 'application/json',
    },
    {
      testCaseId: 'TC-CHECKOUT-001',
      filename: 'checkout-trace.log',
      buffer: Buffer.from('[2026-09-08 10:14:02] INFO: Checkout initiated\n[2026-09-08 10:14:03] INFO: Order #8912 created successfully.'),
      type: 'text/plain',
    },
    {
      testCaseId: 'TC-PAY-001',
      filename: '3ds-challenge-flow.txt',
      buffer: Buffer.from('3DS Step 1: Fingerprint verified\n3DS Step 2: Challenge required\n3DS Step 3: OTP validated'),
      type: 'text/plain',
    },
  ];

  for (const item of attachmentsToUpload) {
    const formData = new FormData();
    const blob = new Blob([item.buffer], { type: item.type });
    formData.append('file', blob, item.filename);

    const res = await api(`/test_cases/${encodeURIComponent(item.testCaseId)}/attachments`, {
      method: 'POST',
      body: formData,
    });
    console.log(`  ✓ Attachment: ${item.filename} on ${item.testCaseId} (${res.status})`);
  }

  // ==========================================
  // 3. TEST SUITES
  // ==========================================
  console.log('\n📦 Creating Test Suites...');
  const testSuites = [
    {
      suiteId: 'SUITE-AUTH-FLOWS.json',
      name: 'SUITE-AUTH-FLOWS',
      description: 'Authentication, registration, SSO OAuth2, and session security.',
      testCases: testCases.filter((tc) => ['TC-AUTH-001', 'TC-AUTH-002', 'TC-AUTH-003'].includes(tc.testCaseId)),
      tags: ['auth', 'security', 'iam'],
    },
    {
      suiteId: 'SUITE-CART-CHECKOUT.json',
      name: 'SUITE-CART-CHECKOUT',
      description: 'Shopping cart operations, coupons, and end-to-end checkout.',
      testCases: testCases.filter((tc) => ['TC-CART-001', 'TC-CART-002', 'TC-CHECKOUT-001'].includes(tc.testCaseId)),
      tags: ['ecommerce', 'cart', 'checkout'],
    },
    {
      suiteId: 'SUITE-PAYMENT-GATEWAY.json',
      name: 'SUITE-PAYMENT-GATEWAY',
      description: 'Credit cards, 3DS authentication, and refund processing.',
      testCases: testCases.filter((tc) => ['TC-PAY-001', 'TC-PAY-002'].includes(tc.testCaseId)),
      tags: ['payments', 'finance', 'gateway'],
    },
    {
      suiteId: 'SUITE-SEARCH-CATALOG.json',
      name: 'SUITE-SEARCH-CATALOG',
      description: 'Product search, filtering, faceted search, and input sanitization.',
      testCases: testCases.filter((tc) => ['TC-SEARCH-001', 'TC-SEARCH-002'].includes(tc.testCaseId)),
      tags: ['search', 'catalog'],
    },
    {
      suiteId: 'SUITE-INVENTORY-LOGISTICS.json',
      name: 'SUITE-INVENTORY-LOGISTICS',
      description: 'Real-time stock reservation and warehouse ERP webhooks.',
      testCases: testCases.filter((tc) => ['TC-INV-001'].includes(tc.testCaseId)),
      tags: ['inventory', 'logistics'],
    },
    {
      suiteId: 'SUITE-MOBILE-RESPONSIVE.json',
      name: 'SUITE-MOBILE-RESPONSIVE',
      description: 'Touch gestures, mobile viewports, and responsive layout fidelity.',
      testCases: testCases.filter((tc) => ['TC-MOB-001', 'TC-CART-001'].includes(tc.testCaseId)),
      tags: ['mobile', 'responsive', 'ui'],
    },
  ];

  for (const ts of testSuites) {
    const res = await api('/test_suites', {
      method: 'POST',
      body: JSON.stringify(ts),
    });
    console.log(`  ✓ Test Suite: ${ts.name} (${ts.testCases.length} cases) (${res.status})`);
  }

  // ==========================================
  // 4. PROJECTS
  // ==========================================
  console.log('\n📂 Creating Projects...');
  const projects = [
    {
      projectId: 'PRJ-ECOMMERCE-STOREFRONT.json',
      name: 'PRJ-ECOMMERCE-STOREFRONT',
      description: 'Core e-commerce storefront web and mobile experience.',
      testSuites: testSuites.filter((s) => ['SUITE-AUTH-FLOWS', 'SUITE-CART-CHECKOUT', 'SUITE-SEARCH-CATALOG', 'SUITE-MOBILE-RESPONSIVE'].includes(s.name)),
      tags: ['web', 'mobile', 'b2c'],
    },
    {
      projectId: 'PRJ-PAYMENT-SERVICES.json',
      name: 'PRJ-PAYMENT-SERVICES',
      description: 'Payment gateway integrations, tokenization, and settlement APIs.',
      testSuites: testSuites.filter((s) => ['SUITE-PAYMENT-GATEWAY'].includes(s.name)),
      tags: ['backend', 'fintech', 'pci-dss'],
    },
    {
      projectId: 'PRJ-IAM-IDENTITY.json',
      name: 'PRJ-IAM-IDENTITY',
      description: 'Centralized customer authentication and access management.',
      testSuites: testSuites.filter((s) => ['SUITE-AUTH-FLOWS'].includes(s.name)),
      tags: ['auth', 'security', 'infrastructure'],
    },
    {
      projectId: 'PRJ-WAREHOUSE-INVENTORY.json',
      name: 'PRJ-WAREHOUSE-INVENTORY',
      description: 'Warehouse inventory tracking and real-time ERP integration.',
      testSuites: testSuites.filter((s) => ['SUITE-INVENTORY-LOGISTICS'].includes(s.name)),
      tags: ['logistics', 'warehouse', 'erp'],
    },
  ];

  for (const prj of projects) {
    const res = await api('/projects', {
      method: 'POST',
      body: JSON.stringify(prj),
    });
    console.log(`  ✓ Project: ${prj.name} (${prj.testSuites.length} suites) (${res.status})`);
  }

  // ==========================================
  // 5. TEST CONFIGURATIONS (Embedded in Runs)
  // ==========================================
  const configurations = [
    {
      configId: 'CFG-CHROME-WIN',
      name: 'Chrome 128 (Windows 11)',
      browser: 'Chrome 128',
      os: 'Windows 11',
      device: 'Desktop',
      resolution: '1920x1080',
    },
    {
      configId: 'CFG-SAFARI-MAC',
      name: 'Safari 18 (macOS)',
      browser: 'Safari 18',
      os: 'macOS Sequoia',
      device: 'Desktop',
      resolution: '2560x1440',
    },
    {
      configId: 'CFG-FIREFOX-LINUX',
      name: 'Firefox 130 (Ubuntu)',
      browser: 'Firefox 130',
      os: 'Ubuntu 24.04',
      device: 'Desktop',
      resolution: '1920x1080',
    },
    {
      configId: 'CFG-IOS-SAFARI',
      name: 'Mobile Safari (iOS 18)',
      browser: 'Mobile Safari',
      os: 'iOS 18',
      device: 'iPhone 15 Pro',
      resolution: '393x852',
    },
  ];

  // ==========================================
  // 6. TEST RUNS
  // ==========================================
  console.log('\n🚀 Creating Test Runs...');
  const testRuns = [
    {
      testRunId: 'RUN-2026-09-01-SMOKE.json',
      name: 'RUN-2026-09-01-SMOKE',
      timestamp: '2026-09-01T09:00:00Z',
      testCases: testCases.filter((tc) => ['TC-AUTH-001', 'TC-CART-001', 'TC-CHECKOUT-001'].includes(tc.testCaseId)),
      results: [
        {
          testCaseId: 'TC-AUTH-001',
          status: 'Passed',
          timestamp: '2026-09-01T09:05:12Z',
          notes: 'Login completed in 180ms. Session token verified.',
        },
        {
          testCaseId: 'TC-CART-001',
          status: 'Passed',
          timestamp: '2026-09-01T09:06:40Z',
          notes: 'Cart counter incremented correctly.',
        },
        {
          testCaseId: 'TC-CHECKOUT-001',
          status: 'Passed',
          timestamp: '2026-09-01T09:08:15Z',
          notes: 'Order #98213 placed with test Visa card.',
        },
      ],
      tags: ['smoke', 'sprint-24.1', 'automated'],
      configurations: configurations.slice(0, 1),
    },
    {
      testRunId: 'RUN-2026-09-05-REGRESSION.json',
      name: 'RUN-2026-09-05-REGRESSION',
      timestamp: '2026-09-05T14:30:00Z',
      testCases: testCases.slice(0, 8),
      results: [
        { testCaseId: 'TC-AUTH-001', status: 'Passed', timestamp: '2026-09-05T14:35:00Z', notes: 'Quick login.' },
        { testCaseId: 'TC-AUTH-002', status: 'Passed', timestamp: '2026-09-05T14:37:10Z', notes: 'OAuth2 code exchange succeeded.' },
        { testCaseId: 'TC-AUTH-003', status: 'Passed', timestamp: '2026-09-05T14:40:22Z', notes: 'Rate limit HTTP 429 triggered.' },
        { testCaseId: 'TC-CART-001', status: 'Passed', timestamp: '2026-09-05T14:42:00Z', notes: 'Add to cart working.' },
        { testCaseId: 'TC-CART-002', status: 'Failed', timestamp: '2026-09-05T14:45:15Z', notes: 'Promo code discount calculation rounding discrepancy by $0.01.' },
        { testCaseId: 'TC-CHECKOUT-001', status: 'Blocked', timestamp: '2026-09-05T14:47:00Z', notes: 'Sandbox payment gateway endpoint unreachable during execution.' },
        { testCaseId: 'TC-PAY-001', status: 'Retest', timestamp: '2026-09-05T14:50:30Z', notes: '3DS challenge iframe intermittently timed out on Safari.' },
        { testCaseId: 'TC-PAY-002', status: 'Untested', timestamp: '2026-09-05T14:52:00Z', notes: 'Awaiting refund service staging deployment.' },
      ],
      tags: ['regression', 'sprint-24.2', 'nightly'],
      configurations: configurations.slice(0, 3),
    },
    {
      testRunId: 'RUN-2026-09-07-SECURITY.json',
      name: 'RUN-2026-09-07-SECURITY',
      timestamp: '2026-09-07T11:00:00Z',
      testCases: testCases.filter((tc) => ['TC-AUTH-003', 'TC-SEARCH-002', 'TC-PAY-001'].includes(tc.testCaseId)),
      results: [
        { testCaseId: 'TC-AUTH-003', status: 'Passed', timestamp: '2026-09-07T11:05:00Z', notes: 'Brute force attempts blocked as expected.' },
        { testCaseId: 'TC-SEARCH-002', status: 'Passed', timestamp: '2026-09-07T11:08:20Z', notes: 'XSS and SQL payload safely sanitized.' },
        { testCaseId: 'TC-PAY-001', status: 'Passed', timestamp: '2026-09-07T11:12:45Z', notes: '3DS cryptographic signature verified.' },
      ],
      tags: ['security', 'owasp', 'audit'],
      configurations: configurations.slice(1, 2),
    },
    {
      testRunId: 'RUN-2026-09-08-MOBILE.json',
      name: 'RUN-2026-09-08-MOBILE',
      timestamp: '2026-09-08T08:15:00Z',
      testCases: testCases.filter((tc) => ['TC-MOB-001', 'TC-CART-001'].includes(tc.testCaseId)),
      results: [
        { testCaseId: 'TC-MOB-001', status: 'Passed', timestamp: '2026-09-08T08:20:00Z', notes: 'Touch gestures verified on iPhone 15 Pro emulator.' },
        { testCaseId: 'TC-CART-001', status: 'Passed', timestamp: '2026-09-08T08:22:10Z', notes: 'Responsive cart flyout matches Figma design.' },
      ],
      tags: ['mobile', 'responsive', 'ios'],
      configurations: configurations.slice(3, 4),
    },
  ];

  for (const run of testRuns) {
    const res = await api('/test_runs', {
      method: 'POST',
      body: JSON.stringify(run),
    });
    console.log(`  ✓ Test Run: ${run.name} (${run.results?.length || 0} results) (${res.status})`);
  }

  // ==========================================
  // 7. MILESTONES
  // ==========================================
  console.log('\n🎯 Creating Milestones...');
  const milestones = [
    {
      milestoneId: 'MS-V1-RELEASE.json',
      name: 'MS-V1-RELEASE',
      description: 'Initial Production Release for Core Storefront & Auth.',
      startDate: '2026-08-15',
      targetDate: '2026-09-01',
      status: 'Completed',
      testSuiteIds: ['SUITE-AUTH-FLOWS.json', 'SUITE-CART-CHECKOUT.json'],
      testRunIds: ['RUN-2026-09-01-SMOKE.json'],
    },
    {
      milestoneId: 'MS-V1-1-SPRINT24.json',
      name: 'MS-V1-1-SPRINT24',
      description: 'Sprint 24 Quality Gate: Search, Payments & Security.',
      startDate: '2026-09-02',
      targetDate: '2026-09-16',
      status: 'In Progress',
      testSuiteIds: ['SUITE-PAYMENT-GATEWAY.json', 'SUITE-SEARCH-CATALOG.json'],
      testRunIds: ['RUN-2026-09-05-REGRESSION.json', 'RUN-2026-09-07-SECURITY.json'],
    },
    {
      milestoneId: 'MS-V2-EXPANSION.json',
      name: 'MS-V2-EXPANSION',
      description: 'Mobile App Launch & Multi-Region Inventory Logistics.',
      startDate: '2026-09-20',
      targetDate: '2026-10-31',
      status: 'Open',
      testSuiteIds: ['SUITE-MOBILE-RESPONSIVE.json', 'SUITE-INVENTORY-LOGISTICS.json'],
      testRunIds: ['RUN-2026-09-08-MOBILE.json'],
    },
    {
      milestoneId: 'MS-Q3-SECURITY-AUDIT.json',
      name: 'MS-Q3-SECURITY-AUDIT',
      description: 'Quarterly SOC 2 & PCI-DSS Compliance Hardening Audit.',
      startDate: '2026-09-01',
      targetDate: '2026-09-30',
      status: 'Open',
      testSuiteIds: ['SUITE-AUTH-FLOWS.json', 'SUITE-PAYMENT-GATEWAY.json'],
      testRunIds: ['RUN-2026-09-07-SECURITY.json'],
    },
  ];

  for (const ms of milestones) {
    const res = await api('/milestones', {
      method: 'POST',
      body: JSON.stringify(ms),
    });
    console.log(`  ✓ Milestone: ${ms.name} (Status: ${ms.status}) (${res.status})`);
  }

  console.log('\n🎉 Seed completed successfully! You can now explore the GUI with rich test data.\n');
}

runSeed().catch((err) => {
  console.error('❌ Failed to seed data:', err);
  process.exit(1);
});
