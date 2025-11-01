import fs from 'fs';
import path from 'path';
import * as vega from 'vega';
import * as vl from 'vega-lite';

// API Configuration
const LIST_API_URL = 'https://api.unstoppableswap.net/api/list';
const LIQUIDITY_DAILY_API_URL = 'https://api.unstoppableswap.net/api/liquidity-daily';
const PROVIDER_QUOTE_STATS_API_URL = 'https://api.unstoppableswap.net/api/provider-quote-stats';
const PROVIDER_DAILY_SWAP_BOUNDS_API_URL = 'https://api.unstoppableswap.net/api/provider-daily-swap-bounds';
const GITHUB_CORE_API_BASE = 'https://api.github.com/repos/eigenwallet/core';
const GITHUB_GUI_API_BASE = 'https://api.github.com/repos/eigenwallet/unstoppableswap-gui';
const GITHUB_CORE_RELEASES_API = `${GITHUB_CORE_API_BASE}/releases`;
const GITHUB_GUI_RELEASES_API = `${GITHUB_GUI_API_BASE}/releases`;

// Cache Configuration
const CACHE_FILE_NAME = '.stats-cache.json';
const CACHE_FILE = path.join(process.cwd(), CACHE_FILE_NAME);
const CACHE_DURATION_MS = 10 * 60 * 1000; // 10 minutes

// Constants
const SATOSHIS_PER_BTC = 100000000;

// Data Interfaces
interface PeerData {
  peerId: string;
  multiAddr: string;
  testnet: boolean;
  version: string;
  price: number; // in satoshis
  minSwapAmount: number; // in satoshis
  maxSwapAmount: number; // in satoshis
  age: number;
  recommended: boolean;
  relevancy: number;
}

interface LiquidityDayData {
  date: number[]; // [year, day_of_year, ...]
  totalLiquidityBtc: number; // already in BTC
}

interface ProviderQuoteStats {
  peer_id: string;
  multi_address: string;
  max_max_swap_amount: number; // in satoshis
  min_min_swap_amount: number; // in satoshis
  online_days: number;
  age_days: number;
  last_seen_ago_days: number;
}

interface ProviderDailySwapBounds {
  day: string; // YYYY-MM-DD format
  peer_id: string;
  daily_max_max_swap_amount: number; // in satoshis
  daily_min_min_swap_amount: number; // in satoshis
}

interface StatsData {
  totalLiquidity: number | null; // in BTC
  maxSwap: number | null; // in BTC
  minSwap: number | null; // in BTC
  totalDownloads: number; // total GitHub downloads
  liquidityChart: string; // SVG string
  leaderboardRecent: string; // HTML leaderboard (recent)
  leaderboardAllTime: string; // HTML leaderboard (all time)
  lastUpdated: string;
}

interface CachedData {
  timestamp: number;
  data: {
    peers: PeerData[] | null;
    liquidity: LiquidityDayData[] | null;
    totalDownloads: number;
    providerQuoteStats: ProviderQuoteStats[] | null;
    providerDailySwapBounds: ProviderDailySwapBounds[] | null;
  };
}

/**
 * Check if we're in development mode
 */
function isDevelopmentMode(): boolean {
  return process.env.NODE_ENV === 'development' || 
         process.env.NODE_ENV === 'dev' ||
         process.argv.includes('--dev') ||
         process.argv.includes('--development');
}

/**
 * Load cached data if available and valid
 */
function loadCache(): { peers: PeerData[] | null; liquidity: LiquidityDayData[] | null; totalDownloads: number; providerQuoteStats: ProviderQuoteStats[] | null; providerDailySwapBounds: ProviderDailySwapBounds[] | null; } | null {
  if (!fs.existsSync(CACHE_FILE)) {
    return null;
  }

  try {
    const cacheContent = fs.readFileSync(CACHE_FILE, 'utf8');
    const cached: CachedData = JSON.parse(cacheContent);
    
    const now = Date.now();
    if (now - cached.timestamp < CACHE_DURATION_MS) {
      console.log('Using cached statistics data...');
      // Ensure backward compatibility for cache that might not have new fields
      return {
        peers: cached.data.peers,
        liquidity: cached.data.liquidity,
        totalDownloads: cached.data.totalDownloads,
        providerQuoteStats: cached.data.providerQuoteStats || null,
        providerDailySwapBounds: cached.data.providerDailySwapBounds || null
      };
    } else {
      console.log('Statistics cache expired, fetching fresh data...');
      return null;
    }
  } catch (error) {
    console.log('Statistics cache corrupted, fetching fresh data...');
    return null;
  }
}

/**
 * Save data to cache
 */
function saveCache(data: { peers: PeerData[] | null; liquidity: LiquidityDayData[] | null; totalDownloads: number; providerQuoteStats: ProviderQuoteStats[] | null; providerDailySwapBounds: ProviderDailySwapBounds[] | null; }): void {
  const cached: CachedData = {
    timestamp: Date.now(),
    data
  };
  
  try {
    fs.writeFileSync(CACHE_FILE, JSON.stringify(cached, null, 2));
    console.log('Statistics data cached successfully');
  } catch (error) {
    console.warn('Failed to save statistics cache:', error);
  }
}

/**
 * Fetch API data
 */
async function fetchApiData(): Promise<{ peers: PeerData[] | null; liquidity: LiquidityDayData[] | null; totalDownloads: number; providerQuoteStats: ProviderQuoteStats[] | null; providerDailySwapBounds: ProviderDailySwapBounds[] | null; }> {
  console.log('Fetching statistics data from APIs...');

  try {
    const [peersResponse, liquidityResponse, providerQuoteStatsResponse, providerDailySwapBoundsResponse, totalDownloads] = await Promise.all([
      fetch(LIST_API_URL),
      fetch(LIQUIDITY_DAILY_API_URL),
      fetch(PROVIDER_QUOTE_STATS_API_URL),
      fetch(PROVIDER_DAILY_SWAP_BOUNDS_API_URL),
      fetchTotalDownloads()
    ]);

    let peers: PeerData[] | null = null;
    let liquidity: LiquidityDayData[] | null = null;
    let providerQuoteStats: ProviderQuoteStats[] | null = null;
    let providerDailySwapBounds: ProviderDailySwapBounds[] | null = null;

    if (!peersResponse.ok) {
      console.warn(`List API responded with status: ${peersResponse.status}`);
    } else {
      try {
        peers = await peersResponse.json();
      } catch (error) {
        console.warn('Failed to parse peers response:', error);
      }
    }

    if (!liquidityResponse.ok) {
      console.warn(`Liquidity API responded with status: ${liquidityResponse.status}`);
    } else {
      try {
        liquidity = await liquidityResponse.json();
      } catch (error) {
        console.warn('Failed to parse liquidity response:', error);
      }
    }

    if (!providerQuoteStatsResponse.ok) {
      console.warn(`Provider quote stats API responded with status: ${providerQuoteStatsResponse.status}`);
    } else {
      try {
        providerQuoteStats = await providerQuoteStatsResponse.json();
      } catch (error) {
        console.warn('Failed to parse provider quote stats response:', error);
      }
    }

    if (!providerDailySwapBoundsResponse.ok) {
      console.warn(`Provider daily swap bounds API responded with status: ${providerDailySwapBoundsResponse.status}`);
    } else {
      try {
        providerDailySwapBounds = await providerDailySwapBoundsResponse.json();
      } catch (error) {
        console.warn('Failed to parse provider daily swap bounds response:', error);
      }
    }

    return { peers, liquidity, totalDownloads, providerQuoteStats, providerDailySwapBounds };
  } catch (error) {
    console.warn('Failed to fetch API data:', error);
    return {
      peers: null,
      liquidity: null,
      totalDownloads: 0,
      providerQuoteStats: null,
      providerDailySwapBounds: null
    };
  }
}

/**
 * Fetch total downloads from GitHub releases (both core and GUI repositories)
 */
async function fetchTotalDownloads(): Promise<number> {
  try {
    console.log('Fetching GitHub download statistics from both repositories...');
    
    // Fetch from both repositories in parallel
    const [coreResponse, guiResponse] = await Promise.all([
      fetch(GITHUB_CORE_RELEASES_API),
      fetch(GITHUB_GUI_RELEASES_API)
    ]);
    
    let totalDownloads = 0;
    
    // Process core repository downloads
    if (coreResponse.ok) {
      const coreReleases: any[] = await coreResponse.json();
      for (const release of coreReleases) {
        if (release.assets && Array.isArray(release.assets)) {
          for (const asset of release.assets) {
            if (asset.download_count) {
              totalDownloads += asset.download_count;
            }
          }
        }
      }
      console.log('Fetched core repository download statistics');
    } else {
      console.warn(`Core GitHub API responded with status: ${coreResponse.status}`);
    }
    
    // Process GUI repository downloads
    if (guiResponse.ok) {
      const guiReleases: any[] = await guiResponse.json();
      for (const release of guiReleases) {
        if (release.assets && Array.isArray(release.assets)) {
          for (const asset of release.assets) {
            if (asset.download_count) {
              totalDownloads += asset.download_count;
            }
          }
        }
      }
      console.log('Fetched GUI repository download statistics');
    } else {
      console.warn(`GUI GitHub API responded with status: ${guiResponse.status}`);
    }

    return totalDownloads;
  } catch (error) {
    console.warn('Failed to fetch GitHub download statistics:', error);
    return 0;
  }
}

/**
 * Convert satoshis to BTC
 */
function satoshisToBtc(satoshis: number): number {
  return satoshis / SATOSHIS_PER_BTC;
}

/**
 * Format number with appropriate precision
 */
function formatNumber(value: number | null | undefined, decimals: number = 4): string {
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return '?';
  }

  if (value >= 1) {
    return value.toFixed(Math.min(decimals, 2));
  }
  return value.toFixed(decimals);
}

/**
 * Format large numbers with k, M, B suffixes and one decimal place
 */
function formatLargeNumber(value: number): string {
  if (value >= 1000000000) {
    return (value / 1000000000).toFixed(1) + 'B';
  } else if (value >= 1000000) {
    return (value / 1000000).toFixed(1) + 'M';
  } else if (value >= 1000) {
    return (value / 1000).toFixed(1) + 'k';
  } else {
    return value.toString();
  }
}

/**
 * Convert date array to readable date
 */
function formatDate(dateArray: number[]): string {
  const year = dateArray[0];
  const dayOfYear = dateArray[1];
  
  // Convert day of year to Date
  const date = new Date(year, 0, dayOfYear);
  return date.toLocaleDateString('en-US', { 
    month: 'short', 
    day: 'numeric'
  });
}

/**
 * Format days since last activity into a human readable string
 */
function formatDaysAgo(days: number | null | undefined): string {
  if (days === null || days === undefined || Number.isNaN(days)) {
    return 'Unknown';
  }
  if (days < 0) {
    return 'Unknown';
  }
  if (days === 0) {
    return 'today';
  }
  if (days === 1) {
    return '1 day ago';
  }
  return `${days} days ago`;
}

/**
 * Generate leaderboard HTML for top 10 makers
 */
function generateLeaderboardSections(providerQuoteStats: ProviderQuoteStats[] | null): { recentSection: string; allTimeSection: string; } {
  if (!providerQuoteStats || providerQuoteStats.length === 0) {
    return {
      recentSection: `
        <div id="recently-seen" style="margin: 2rem 0;">
          <div style="background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 2px 4px rgba(0,0,0,0.1);">
            <div style="padding: 1.5rem; color: #6c757d;">No provider data available.</div>
          </div>
        </div>`,
      allTimeSection: `
        <div id="all-time" style="margin: 2rem 0;">
          <div style="background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 2px 4px rgba(0,0,0,0.1);">
            <div style="padding: 1.5rem; color: #6c757d;">No provider data available.</div>
          </div>
        </div>`
    };
  }

  const allMakersByOnline = [...providerQuoteStats].sort((a, b) => b.online_days - a.online_days);
  const allVisibleMakers = allMakersByOnline.filter(maker => (maker.online_days ?? 0) > 1);
  const showAllRedactedNote = allVisibleMakers.length < allMakersByOnline.length;

  const recentlySeenCandidates = [...providerQuoteStats]
    .filter(maker => maker.last_seen_ago_days !== undefined && maker.last_seen_ago_days < 7)
    .sort((a, b) => {
      const aValue = a.last_seen_ago_days ?? Number.POSITIVE_INFINITY;
      const bValue = b.last_seen_ago_days ?? Number.POSITIVE_INFINITY;
      if (aValue !== bValue) {
        return aValue - bValue;
      }
      return b.online_days - a.online_days;
    });
  const recentlySeenMakers = recentlySeenCandidates.filter(maker => (maker.online_days ?? 0) > 1);
  const showRecentRedactedNote = recentlySeenMakers.length < recentlySeenCandidates.length;

  const renderLastSeen = (maker: any): string => {
    if (maker.last_seen_ago_days !== undefined && maker.last_seen_ago_days < 1) {
      return '<span style="color: #28a745; font-weight: 600;">Recently</span>';
    }
    return `<span>${formatDaysAgo(maker.last_seen_ago_days)}</span>`;
  };

  const buildTable = (makers: any[], showRedactedNote: boolean): string => {
    let tableHtml = `
        <div style="overflow-x: auto; -webkit-overflow-scrolling: touch;">
          <table style="width: 100%; min-width: 640px; border-collapse: collapse;">
            <thead>
              <tr style="background: #f8f9fa;">
                <th style="padding: 1rem; text-align: left; border-bottom: 2px solid #dee2e6; font-weight: 600; color: #495057;">Address</th>
                <th style="padding: 1rem; text-align: right; border-bottom: 2px solid #dee2e6; font-weight: 600; color: #495057;">Online Days</th>
                <th style="padding: 1rem; text-align: right; border-bottom: 2px solid #dee2e6; font-weight: 600; color: #495057;">Max Swap</th>
                <th style="padding: 1rem; text-align: right; border-bottom: 2px solid #dee2e6; font-weight: 600; color: #495057;">Min Swap</th>
                <th style="padding: 1rem; text-align: right; border-bottom: 2px solid #dee2e6; font-weight: 600; color: #495057;">Last Seen</th>
              </tr>
            </thead>
            <tbody>`;

    makers.forEach((maker, index) => {
      const maxSwapBtc = formatNumber(satoshisToBtc(maker.max_max_swap_amount));
      const minSwapBtc = formatNumber(satoshisToBtc(maker.min_min_swap_amount));
      const displayAddress = maker.multi_address.length > 30 
        ? maker.multi_address.substring(0, 30) + '...'
        : maker.multi_address;

      tableHtml += `
              <tr style="border-bottom: 1px solid #dee2e6; ${index % 2 === 0 ? 'background: #f8f9fa;' : 'background: white;'}">
                <td style="padding: 1rem; color: #495057; font-family: monospace; font-size: 0.9rem;" title="${maker.multi_address}">
                  <a href="peer/${maker.peer_id}.html" style="color: #ff6b35; text-decoration: none; font-weight: 600;">${displayAddress}</a>
                </td>
                <td style="padding: 1rem; text-align: right; font-weight: 600; color: #28a745;">${maker.online_days} days</td>
                <td style="padding: 1rem; text-align: right; color: #495057;">${maxSwapBtc} BTC</td>
                <td style="padding: 1rem; text-align: right; color: #495057;">${minSwapBtc} BTC</td>
                <td style="padding: 1rem; text-align: right; color: #495057;">${renderLastSeen(maker)}</td>
              </tr>`;
    });

    if (showRedactedNote) {
      tableHtml += `
              <tr style="background: white;">
                <td colspan="5" style="padding: 1rem; text-align: center; color: #6c757d; font-style: italic;">Others redacted for clarity</td>
              </tr>`;
    }

    tableHtml += `
            </tbody>
          </table>
        </div>`;

    return tableHtml;
  };

  const renderSection = (titleId: string, makers: any[], emptyMessage: string, showRedactedNote: boolean): string => `
    <div id="${titleId}" style="margin: 2rem 0;">
      <div style="background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 2px 4px rgba(0,0,0,0.1);">
        ${makers.length > 0 ? buildTable(makers, showRedactedNote) : `<div style="padding: 1.5rem; color: #6c757d;">${emptyMessage}</div>`}
      </div>
    </div>`;

  return {
    recentSection: renderSection('recently-seen', recentlySeenMakers, 'No data', showRecentRedactedNote),
    allTimeSection: renderSection('all-time', allVisibleMakers, 'No maker data available.', showAllRedactedNote)
  };
}

/**
 * Generate historical chart for a specific maker using Vega-Lite
 */
async function generateMakerHistoricalChart(peerId: string, providerDailySwapBounds: ProviderDailySwapBounds[] | null): Promise<string> {
  if (!providerDailySwapBounds) {
    return `<svg width="800" height="200" viewBox="0 0 800 200">
      <text x="400" y="100" text-anchor="middle" fill="#666">No historical data available</text>
    </svg>`;
  }

  // Filter historical data for this specific maker
  const makerHistoricalData = providerDailySwapBounds.filter(data => data.peer_id === peerId);
  
  if (makerHistoricalData.length === 0) {
    return `<svg width="800" height="200" viewBox="0 0 800 200">
      <text x="400" y="100" text-anchor="middle" fill="#666">No historical data available</text>
    </svg>`;
  }

  // Transform data for Vega-Lite
  const chartData = makerHistoricalData.map(d => ({
    date: d.day,
    maxSwap: satoshisToBtc(d.daily_max_max_swap_amount || 0),
  })).sort((a, b) => new Date(a.date).getTime() - new Date(b.date).getTime());

  // Vega-Lite specification for dual line chart
  const spec: vl.TopLevelSpec = {
    $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
    width: 750,
    height: 400,
    background: 'transparent',
    padding: { left: 0, right: 0, top: 0, bottom: 0 },
    data: {
      values: chartData
    },
    layer: [
      // Max swap line
      {
        mark: {
          type: 'line',
          color: '#ff6b35',
          strokeWidth: 3,
          strokeCap: 'round',
          strokeJoin: 'round'
        },
        encoding: {
          x: {
            title: null,
            field: 'date',
            type: 'temporal',
            axis: {
              format: '%b %d',
              labelAngle: 0,
              labelFontSize: 16,
              labelColor: '#666',
              tickColor: 'transparent',
              domainColor: 'transparent',
              grid: false
            }
          },
          y: {
            title: null,
            field: 'maxSwap',
            type: 'quantitative',
            axis: {
              labelFontSize: 16,
              labelColor: '#666',
              tickColor: 'transparent',
              domainColor: 'transparent',
              grid: false,
              format: '.3f',
              labelExpr: 'format(datum.value, ".3f") + " BTC"'
            }
          }
        }
      },
    ],
    resolve: {
      scale: { y: 'shared', x: 'shared' }
    }
  };

  try {
    // Compile Vega-Lite to Vega
    const vegaSpec = vl.compile(spec).spec;
    
    // Create Vega view and render to SVG
    const view = new vega.View(vega.parse(vegaSpec), { renderer: 'none' });
    const svg = await view.toSVG();
    
    // Add responsive styling to the SVG
    return svg.replace('<svg', '<svg style="max-width: 100%; height: auto;"');
  } catch (error) {
    console.error('Failed to generate maker chart:', error);
    return `<svg width="800" height="200" viewBox="0 0 800 200">
      <text x="400" y="100" text-anchor="middle" fill="#666">Chart generation failed</text>
    </svg>`;
  }
}

/**
 * Generate individual maker page HTML
 */
async function generateMakerPage(peerId: string, providerQuoteStats: ProviderQuoteStats[] | null, providerDailySwapBounds: ProviderDailySwapBounds[] | null): Promise<string> {
  // Find maker data
  const maker = providerQuoteStats?.find(m => m.peer_id === peerId);
  if (!maker) {
    return `<!DOCTYPE html>
<html>
<head>
  <title>Maker Not Found</title>
  <link rel="stylesheet" href="latex.min.css">
</head>
<body>
  <h1>Maker Not Found</h1>
  <p>The requested maker (${peerId}) was not found.</p>
</body>
</html>`;
  }

  // Generate historical chart
  const historicalChart = await generateMakerHistoricalChart(peerId, providerDailySwapBounds);

  // Get historical data for stats
  const historicalData = providerDailySwapBounds?.filter(data => data.peer_id === peerId) || [];
  const latestData = historicalData.length > 0 ? historicalData[historicalData.length - 1] : null;

  const formatDaysAgo = (days: number | null | undefined): string => {
    if (days === null || days === undefined || Number.isNaN(days)) {
      return 'Unknown';
    }
    if (days < 0) {
      return 'Unknown';
    }
    if (days === 0) {
      return 'today';
    }
    if (days === 1) {
      return '1 day ago';
    }
    return `${days} days ago`;
  };

  const lastSeenContent = maker.last_seen_ago_days !== undefined && maker.last_seen_ago_days < 1
    ? '<span style="color: #28a745; font-weight: 600;">Online</span>'
    : `<span>${formatDaysAgo(maker.last_seen_ago_days)}</span>`;

  const firstSeenContent = `<span>${formatDaysAgo(maker.age_days)}</span>`;

  return `<!DOCTYPE html>
<html>
<head>
  <title>Maker ${peerId}</title>
  <link rel="stylesheet" href="latex.min.css">
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <style>
    body { margin: 0; background: #f8f9fa; color: #212529; }
    main { padding: 2rem 1.5rem; max-width: 1200px; margin: 0 auto; box-sizing: border-box; }
    .maker-layout { display: flex; flex-direction: column; gap: 2rem; }
    .maker-card { background: white; border: 1px solid #dee2e6; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.08); }
    .table-card { overflow: hidden; }
    .chart-card { padding: 1.5rem; }
    @media (min-width: 992px) {
      .maker-layout { flex-direction: row; align-items: flex-start; }
      .maker-layout > section { flex: 1; }
      .maker-layout-chart { flex: 1.2; }
    }
  </style>
</head>
<body>
  <header>
    <nav>
      <a href="../statistics.html">Back</a>
    </nav>
  </header>

  <main>
    <div class="maker-layout">
      <section class="maker-layout-table">
        <div class="maker-card table-card">
          <table style="width: 100%; border-collapse: collapse; margin: 0; background: white;">
            <thead>
              <tr style="background: #f8f9fa;">
                <th style="padding: 1rem; text-align: left; border-bottom: 2px solid #dee2e6; font-weight: 600; color: #495057;">Metric</th>
                <th style="padding: 1rem; text-align: right; border-bottom: 2px solid #dee2e6; font-weight: 600; color: #495057;">Value</th>
              </tr>
            </thead>
            <tbody>
              <tr style="border-bottom: 1px solid #dee2e6;">
                <td style="padding: 1rem; text-align: left; font-weight: 600; color: #495057; width: 30%;">Public Key</td>
                <td style="padding: 1rem; text-align: right; color: #495057; font-family: monospace; font-size: 0.9rem; word-break: break-all;">${maker.peer_id}</td>
              </tr>
              <tr style="border-bottom: 1px solid #dee2e6;">
                <td style="padding: 1rem; text-align: left; font-weight: 600; color: #495057; width: 30%;">Address</td>
                <td style="padding: 1rem; text-align: right; color: #495057; font-family: monospace; font-size: 0.9rem; word-break: break-all;">${maker.multi_address}</td>
              </tr>
              <tr style="border-bottom: 1px solid #dee2e6;">
                <td style="padding: 1rem; text-align: left; font-weight: 600; color: #495057;">Online Days</td>
                <td style="padding: 1rem; text-align: right; color: #28a745; font-weight: 600;">${maker.online_days} days</td>
              </tr>
              <tr style="border-bottom: 1px solid #dee2e6;">
                <td style="padding: 1rem; text-align: left; font-weight: 600; color: #495057;">First seen</td>
                <td style="padding: 1rem; text-align: right; color: #495057; font-weight: 500;">${firstSeenContent}</td>
              </tr>
              <tr style="border-bottom: 1px solid #dee2e6;">
                <td style="padding: 1rem; text-align: left; font-weight: 600; color: #495057;">Last seen</td>
                <td style="padding: 1rem; text-align: right; color: #495057; font-weight: 500;">${lastSeenContent}</td>
              </tr>
              <tr style="border-bottom: 1px solid #dee2e6;">
                <td style="padding: 1rem; text-align: left; font-weight: 600; color: #495057;">Largest offer (all time)</td>
                <td style="padding: 1rem; text-align: right; color: #ff6b35; font-weight: 600;">${formatNumber(satoshisToBtc(maker.max_max_swap_amount))} BTC</td>
              </tr>
              <tr>
                <td style="padding: 1rem; text-align: left; font-weight: 600; color: #495057;">Smallest offer (all time)</td>
                <td style="padding: 1rem; text-align: right; color: #ff6b35; font-weight: 600;">${formatNumber(satoshisToBtc(maker.min_min_swap_amount))} BTC</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <section class="maker-layout-chart">
        <div class="maker-card chart-card">
          <div style="display: flex; justify-content: center; align-items: center; gap: 2rem; margin-bottom: 1.5rem; flex-wrap: wrap;">
            <div style="display: flex; align-items: center; gap: 0.5rem;">
              <div style="width: 20px; height: 3px; background: #ff6b35;"></div>
              <span>Maximum Swap</span>
            </div>
          </div>
          <div style="text-align: center;">
            ${historicalChart}
          </div>
        </div>
      </section>
    </div>
  </main>
</body>
</html>`;
}

/**
 * Generate SVG chart for liquidity data using Vega-Lite
 */
async function generateLiquidityChart(liquidityData: LiquidityDayData[]): Promise<string> {
  if (liquidityData.length === 0) {
    return `<svg width="800" height="200" viewBox="0 0 800 200">
      <text x="400" y="100" text-anchor="middle" fill="#666">No data available</text>
    </svg>`;
  }

  // Transform data for Vega-Lite
  const chartData = liquidityData.map(d => {
    const year = d.date[0];
    const dayOfYear = d.date[1];
    const date = new Date(year, 0, dayOfYear);
    
    return {
      date: date.toISOString().split('T')[0], // YYYY-MM-DD format
      liquidity: d.totalLiquidityBtc
    };
  }).reverse(); // Reverse to show chronological order (oldest to newest)

  // Vega-Lite specification
  const spec: vl.TopLevelSpec = {
    $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
    width: 750,
    height: 500,
    background: 'transparent',
    padding: { left: 0, right: 0, top: 0, bottom: 0 },
    data: {
      values: chartData
    },
    layer: [
             // Area chart
       {
         mark: {
           type: 'area',
           color: '#ff6b35',
           opacity: 0.2,
           line: false
         },
        encoding: {
          x: {
            title: null,
            field: 'date',
            type: 'temporal',
            axis: {
              format: '%b %d',
              labelAngle: 0,
              labelFontSize: 18,
              labelColor: '#666',
              tickColor: 'transparent',
              domainColor: 'transparent',
              grid: false
            }
          },
          y: {
            title: null,
             field: 'liquidity',
             type: 'quantitative',
             axis: {
               labelFontSize: 18,
               labelColor: '#666',
               tickColor: 'transparent',
               domainColor: 'transparent',
               grid: false,
               format: '.3f',
               labelExpr: 'datum.value == 0 ? "" : format(datum.value, ".0f") + " BTC"',
               titleFontSize: 18,
               titleColor: '#666'
             }
           }
        }
      },
      // Line chart
      {
        mark: {
          type: 'line',
          color: '#ff6b35',
          strokeWidth: 2,
          strokeCap: 'round',
          strokeJoin: 'round'
        },
        encoding: {
          x: {
            field: 'date',
            type: 'temporal'
          },
          y: {
            field: 'liquidity',
            type: 'quantitative'
          }
        }
      }
    ],
    resolve: {
      scale: { y: 'shared', x: 'shared' }
    }
  };

  try {
    // Compile Vega-Lite to Vega
    const vegaSpec = vl.compile(spec).spec;
    
    // Create Vega view and render to SVG
    const view = new vega.View(vega.parse(vegaSpec), { renderer: 'none' });
    const svg = await view.toSVG();
    
    // Add responsive styling to the SVG
    return svg.replace('<svg', '<svg style="max-width: 100%; height: auto;"');
  } catch (error) {
    console.error('Failed to generate chart:', error);
    return `<svg width="800" height="200" viewBox="0 0 800 200">
      <text x="400" y="100" text-anchor="middle" fill="#666">Chart generation failed</text>
    </svg>`;
  }
}

/**
 * Generate statistics data
 */
export async function generateStatsData(): Promise<StatsData> {
  let apiData: { peers: PeerData[] | null; liquidity: LiquidityDayData[] | null; totalDownloads: number; providerQuoteStats: ProviderQuoteStats[] | null; providerDailySwapBounds: ProviderDailySwapBounds[] | null; };

  // Try cache in development mode
  if (isDevelopmentMode()) {
    const cachedData = loadCache();
    if (cachedData) {
      console.log(`Using cached downloads: ${cachedData.totalDownloads}`);
      apiData = cachedData;
    } else {
      apiData = await fetchApiData();
      console.log(`Caching fresh downloads: ${apiData.totalDownloads}`);
      saveCache(apiData);
    }
  } else {
    // Always fetch fresh data in production
    apiData = await fetchApiData();
  }

  const { peers, liquidity, totalDownloads, providerQuoteStats, providerDailySwapBounds } = apiData;

  // Calculate statistics with null handling
  const activePeers = peers ? peers.filter(p => !p.testnet) : [];

  // Total liquidity from the latest daily data (first item = most recent, already in BTC)
  const totalLiquidity = liquidity && liquidity.length > 0 ? liquidity[0].totalLiquidityBtc : null;

  let maxSwap: number | null = null;
  let minSwap: number | null = null;

  if (peers && activePeers.length > 0) {
    const maxSwapSatoshis = Math.max(...activePeers.map(p => p.maxSwapAmount));
    const minSwapSatoshis = Math.min(...activePeers.map(p => p.minSwapAmount));

    maxSwap = satoshisToBtc(maxSwapSatoshis);
    minSwap = satoshisToBtc(minSwapSatoshis);
  }

  // Generate chart only if we have liquidity data
  const liquidityChart = liquidity ? await generateLiquidityChart(liquidity) : `<svg width="800" height="200" viewBox="0 0 800 200">
    <text x="400" y="100" text-anchor="middle" fill="#666">No data</text>
  </svg>`;

  // Generate leaderboard
  const { recentSection: leaderboardRecent, allTimeSection: leaderboardAllTime } = generateLeaderboardSections(providerQuoteStats);

  return {
    totalLiquidity,
    maxSwap,
    minSwap,
    totalDownloads,
    liquidityChart,
    leaderboardRecent,
    leaderboardAllTime,
    lastUpdated: new Date().toISOString().split('T')[0]
  };
}

/**
 * Generate all individual maker pages
 */
export async function generateAllMakerPages(outputDir: string, providerQuoteStats?: ProviderQuoteStats[] | null, providerDailySwapBounds?: ProviderDailySwapBounds[] | null): Promise<void> {
  console.log('Generating individual maker pages...');
  
  let quoteStats = providerQuoteStats;
  let dailySwapBounds = providerDailySwapBounds;
  
  // If data is not provided, fetch it
  if (!quoteStats || !dailySwapBounds) {
    console.log('API data not provided, fetching fresh data for maker pages...');
    const apiData = await fetchApiData();
    quoteStats = apiData.providerQuoteStats;
    dailySwapBounds = apiData.providerDailySwapBounds;
  }
  
  if (!quoteStats || quoteStats.length === 0) {
    console.warn('No provider quote stats available, skipping maker page generation');
    return;
  }
  
  const peerDir = path.join(outputDir, 'peer');
  fs.mkdirSync(peerDir, { recursive: true });

  for (const maker of quoteStats) {
    try {
      const makerPageHtml = await generateMakerPage(maker.peer_id, quoteStats, dailySwapBounds);
      const outputPath = path.join(peerDir, `${maker.peer_id}.html`);
      fs.writeFileSync(outputPath, makerPageHtml);
      console.log(`Generated: ${maker.peer_id}.html`);
    } catch (error) {
      console.error(`Failed to generate page for ${maker.peer_id}:`, error);
    }
  }
  
  console.log('All maker pages generated successfully');
}

/**
 * Process statistics template
 */
export async function processStatsTemplate(template: string): Promise<string> {
  const statsData = await generateStatsData();

  return template
    .replace(/\{\{TOTAL_LIQUIDITY\}\}/g, statsData.totalLiquidity !== null ? formatNumber(statsData.totalLiquidity) : '?')
    .replace(/\{\{MAX_SWAP\}\}/g, statsData.maxSwap !== null ? formatNumber(statsData.maxSwap) : '?')
    .replace(/\{\{MIN_SWAP\}\}/g, statsData.minSwap !== null ? formatNumber(statsData.minSwap) : '?')
    .replace(/\{\{TOTAL_DOWNLOADS\}\}/g, formatLargeNumber(statsData.totalDownloads))
    .replace(/\{\{LIQUIDITY_CHART\}\}/g, statsData.liquidityChart)
    .replace(/\{\{LEADERBOARD_RECENT\}\}/g, statsData.leaderboardRecent)
    .replace(/\{\{LEADERBOARD_ALLTIME\}\}/g, statsData.leaderboardAllTime)
    .replace(/\{\{LAST_UPDATED\}\}/g, statsData.lastUpdated);
}
