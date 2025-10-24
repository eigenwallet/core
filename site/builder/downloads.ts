import fs from 'fs';
import path from 'path';
import cliProgress from 'cli-progress';

// API and Repository Configuration
const GITHUB_API_BASE = 'https://api.github.com/repos/eigenwallet/core';
const GITHUB_RELEASES_API = `${GITHUB_API_BASE}/releases/latest`;
const GITHUB_RELEASES_BASE = 'https://github.com/eigenwallet/core/releases/download';
const GITHUB_ARCHIVE_BASE = 'https://github.com/eigenwallet/core/archive';
const SIGNING_KEY_URL = 'https://github.com/eigenwallet/core/blob/main/pgp-key.asc';

// Asset Filtering Patterns
const ASSET_PREFIXES = {
  GUI: 'eigenwallet_',
  CLI_ASB: 'asb_',
  CLI_ASB_CONTROLLER: 'asb-controller_',
  CLI_SWAP: 'swap_',
  CLI_ORCHESTRATOR: 'orchestrator_',
  CLI_RENDEZVOUS: 'rendezvous-server_',
} as const;

const FILE_EXTENSIONS = {
  SIGNATURE: '.sig',
  DMG: '.dmg',
  APPIMAGE: '.appimage',
  DEB: '.deb',
  RPM: '.rpm',
  MSI: '.msi',
  EXE: '.exe',
  APP_BUNDLE: '.app.tar.gz',
  TAR: '.tar',
  ZIP: '.zip',
} as const;

// Cache Configuration
const CACHE_FILE_NAME = '.github-cache.json';
const CACHE_FILE = path.join(process.cwd(), CACHE_FILE_NAME);
const CACHE_DURATION_MS = 10 * 60 * 1000; // 10 minutes

// Platform Configuration
const PLATFORM_ORDER = ['Linux', 'Windows', 'macOS', 'Unknown', 'Source archive'] as const;
const PLATFORM_ICONS = {
  Linux: '🐧',
  Windows: '🪟',
  macOS: '🍎',
  Unknown: '❓',
  'Source archive': '📁',
} as const;

// File Size Units
const FILE_SIZE_UNITS = ['B', 'KB', 'MB', 'GB'] as const;

// Template Placeholders
const TEMPLATE_PLACEHOLDERS = {
  VERSION: '{{LATEST_VERSION}}',
  RELEASE_DATE: '{{RELEASE_DATE}}',
  GUI_TABLE: '{{GUI_TABLE}}',
  CLI_TABLE: '{{CLI_TABLE}}',
  AUR_TABLE: '{{AUR_TABLE}}',
  HASHES_LINK: '{{HASHES_LINK}}',
  SIGNING_KEY_LINK: '{{SIGNING_KEY_LINK}}',
} as const;

// Architecture Detection Patterns
const ARCH_PATTERNS = {
  X64: ['x86_64', 'amd64', 'x64'],
  ARM64: ['aarch64', 'arm64'],
  ARM32: ['arm32', 'armv7'],
  ARM: ['arm'],
  X86: ['i386', 'x86'],
} as const;

// Platform Detection Patterns
const PLATFORM_PATTERNS = {
  LINUX: ['linux'],
  MACOS: ['darwin', 'macos'],
  WINDOWS: ['windows', 'win'],
  ANDROID: ['android'],
  IOS: ['ios'],
} as const;

interface DownloadAsset {
  name: string;
  downloadUrl: string;
  signatureUrl: string;
  size: string;
  architecture: string;
  platform: string;
  type: 'executable' | 'appimage' | 'installer' | 'bundle' | 'archive' | 'instructions';
}

interface ReleaseInfo {
  version: string;
  releaseDate: string;
  hashesUrl: string;
  signingKeyUrl: string;
  assets: DownloadAsset[];
}

interface GitHubAsset {
  name: string;
  browser_download_url: string;
  size: number;
}

interface GitHubRelease {
  tag_name: string;
  published_at: string;
  assets: GitHubAsset[];
}

interface CachedData {
  timestamp: number;
  data: GitHubRelease;
}

interface AURPackage {
  name: string;
  packageUrl: string;
  maintainer: string;
  maintainerUrl: string;
  version?: string;
  architectures: string[];
}

const AUR_PACKAGES: AURPackage[] = [
  {
    name: 'eigenwallet-bin',
    packageUrl: 'https://aur.archlinux.org/packages/eigenwallet-bin',
    maintainer: 'Kainoa Kanter (That1Calculator)',
    maintainerUrl: 'https://aur.archlinux.org/account/That1Calculator',
    architectures: ['x86_64'],
  },
  {
    name: 'eigenwallet-developertools-bin',
    packageUrl: 'https://aur.archlinux.org/packages/eigenwallet-developertools-bin',
    maintainer: 'Kainoa Kanter (That1Calculator)',
    maintainerUrl: 'https://aur.archlinux.org/account/That1Calculator',
    architectures: ['x86_64'],
  }
];

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
 * Load cached GitHub data if available and valid
 */
function loadCache(): GitHubRelease | null {
  if (!fs.existsSync(CACHE_FILE)) {
    return null;
  }

  try {
    const cacheContent = fs.readFileSync(CACHE_FILE, 'utf8');
    const cached: CachedData = JSON.parse(cacheContent);

    // Check if cache is still valid (within cache duration)
    const now = Date.now();
    if (now - cached.timestamp < CACHE_DURATION_MS) {
      console.log('Using cached GitHub API data...');
      return cached.data;
    } else {
      console.log('Cache expired, fetching fresh data...');
      return null;
    }
  } catch {
    console.log('Cache file corrupted, fetching fresh data...');
    return null;
  }
}

/**
 * Save GitHub data to cache
 */
function saveCache(data: GitHubRelease): void {
  const cached: CachedData = {
    timestamp: Date.now(),
    data
  };

  try {
    fs.writeFileSync(CACHE_FILE, JSON.stringify(cached, null, 2));
    console.log('GitHub API data cached successfully');
  } catch (error) {
    console.warn('Failed to save cache:', error);
  }
}

/**
 * Fetch latest release data from GitHub API
 */
export async function generateDownloadData(): Promise<ReleaseInfo> {
  let release: GitHubRelease;

  // Try to use cache in development mode
  if (isDevelopmentMode()) {
    const cachedData = loadCache();
    if (cachedData) {
      release = cachedData;
    } else {
      console.log('Fetching fresh GitHub API data...');
      const response = await fetch(GITHUB_RELEASES_API);
      if (!response.ok) {
        throw new Error(`GitHub API responded with status: ${response.status}. Please check your internet connection or GitHub API rate limits.`);
      }

      release = await response.json() as GitHubRelease;
      saveCache(release);
    }
  } else {
    // In production, always fetch fresh data
    const response = await fetch(GITHUB_RELEASES_API);
    if (!response.ok) {
      throw new Error(`GitHub API responded with status: ${response.status}. Please check your internet connection or GitHub API rate limits.`);
    }

    release = await response.json() as GitHubRelease;
  }

  // Filter for relevant wallet assets (exclude signature files .sig and .asc)
  const walletAssets = release.assets.filter(asset =>
    (asset.name.startsWith(ASSET_PREFIXES.GUI) ||
     asset.name.startsWith(ASSET_PREFIXES.CLI_ASB) ||
     asset.name.startsWith(ASSET_PREFIXES.CLI_ASB_CONTROLLER) ||
     asset.name.startsWith(ASSET_PREFIXES.CLI_SWAP) ||
     asset.name.startsWith(ASSET_PREFIXES.CLI_ORCHESTRATOR) ||
     asset.name.startsWith(ASSET_PREFIXES.CLI_RENDEZVOUS)) &&
    !asset.name.endsWith(FILE_EXTENSIONS.SIGNATURE) &&
    !asset.name.endsWith('.asc')
  );

  // Map of asset name to its download URL to resolve matching signature files
  const assetNameToUrl = new Map(release.assets.map(a => [a.name, a.browser_download_url]));

  const assets: DownloadAsset[] = walletAssets.map(asset => {
    const { platform, architecture, type } = parseAssetName(asset.name);
    const signatureUrl = assetNameToUrl.get(`${asset.name}.asc`) || '';

    return {
      name: getDisplayName(asset.name, type),
      downloadUrl: asset.browser_download_url,
      signatureUrl,
      size: formatFileSize(asset.size),
      architecture,
      platform,
      type
    };
  });

  // Add source archive if no specific eigenwallet assets found
  if (assets.length === 0) {
    assets.push({
      name: "Source Code (.tar.gz)",
      downloadUrl: `${GITHUB_ARCHIVE_BASE}/${release.tag_name}.tar.gz`,
      signatureUrl: "",
      size: "~2 MB",
      architecture: "",
      platform: "Source archive",
      type: "archive"
    });
  }

  const version = release.tag_name.replace(/^v/, '');
  const releaseDate = new Date(release.published_at).toISOString().split('T')[0];

  return {
    version,
    releaseDate,
    hashesUrl: `${GITHUB_RELEASES_BASE}/${release.tag_name}/hashes.txt`,
    signingKeyUrl: SIGNING_KEY_URL,
    assets
  };
}

/**
 * Check if asset is a tar file
 */
function isTarFile(assetName: string): boolean {
  return assetName.toLowerCase().includes(FILE_EXTENSIONS.TAR);
}

/**
 * Parse asset name to extract platform, architecture, and type information
 */
function parseAssetName(assetName: string): { platform: string; architecture: string; type: DownloadAsset['type'] } {
  const name = assetName.toLowerCase();
  const isTar = isTarFile(assetName);

  // Determine platform first by explicit keywords, then by file extensions
  let platform = "Unknown";
  if (PLATFORM_PATTERNS.LINUX.some(p => name.includes(p))) platform = "Linux";
  else if (PLATFORM_PATTERNS.MACOS.some(p => name.includes(p))) platform = "macOS";
  else if (PLATFORM_PATTERNS.WINDOWS.some(p => name.includes(p))) platform = "Windows";
  else if (PLATFORM_PATTERNS.ANDROID.some(p => name.includes(p))) platform = "Android";
  else if (PLATFORM_PATTERNS.IOS.some(p => name.includes(p))) platform = "iOS";
  else if (name.includes(FILE_EXTENSIONS.DMG) || name.includes(FILE_EXTENSIONS.APP_BUNDLE)) platform = "macOS";
  else if (name.includes(FILE_EXTENSIONS.APPIMAGE) || name.includes(FILE_EXTENSIONS.DEB) || name.includes(FILE_EXTENSIONS.RPM)) platform = "Linux";
  else if (name.includes(FILE_EXTENSIONS.EXE) || name.includes(FILE_EXTENSIONS.MSI)) platform = "Windows";

  // Determine architecture
  let architecture = "";
  const isAppImage = name.includes(FILE_EXTENSIONS.APPIMAGE);
  const isDebian = name.includes(FILE_EXTENSIONS.DEB);

  if (ARCH_PATTERNS.X64.some(p => name.includes(p))) {
    if (platform === "macOS") {
      const macReleaseType = name.includes(FILE_EXTENSIONS.DMG) ? 'DMG' : name.includes(FILE_EXTENSIONS.APP_BUNDLE) ? 'Bundle' : 'Binary';
      architecture = "Intel <span style='float: right;'>" + macReleaseType + "</span>";
    } else if (isAppImage) {
      architecture = "x86_64 <span style='float: right;'>AppImage</span>";
    } else if (isDebian) {
      architecture = "x86_64 <span style='float: right;'>Debian</span>";
    } else if (isTar) {
      architecture = "x86_64 <span style='float: right;'>Binary</span>";
    } else {
      architecture = "x86_64";
    }
  } else if (ARCH_PATTERNS.ARM64.some(p => name.includes(p))) {
    if (platform === "macOS") {
      const macReleaseType = name.includes(FILE_EXTENSIONS.DMG) ? 'DMG' : name.includes(FILE_EXTENSIONS.APP_BUNDLE) ? 'Bundle' : 'Binary';
      architecture = "Silicon <span style='float: right;'>" + macReleaseType + "</span>";
    } else if (isAppImage) {
      architecture = "AppImage <span style='float: right;'>ARM64</span>";
    } else if (isDebian) {
      architecture = "Debian <span style='float: right;'>ARM64</span>";
    } else if (isTar) {
      architecture = "ARM64 <span style='float: right;'>Binary</span>";
    } else {
      architecture = "ARM64";
    }
  } else if (ARCH_PATTERNS.ARM32.some(p => name.includes(p))) {
    if (isAppImage) {
      architecture = "AppImage <span style='float: right;'>ARM32</span>";
    } else if (isDebian) {
      architecture = "Debian <span style='float: right;'>ARM32</span>";
    } else if (isTar) {
      architecture = "ARM32 <span style='float: right;'>Binary</span>";
    } else {
      architecture = "ARM32";
    }
  } else if (ARCH_PATTERNS.ARM.some(p => name.includes(p))) {
    if (isAppImage) {
      architecture = "AppImage <span style='float: right;'>ARM</span>";
    } else if (isDebian) {
      architecture = "Debian <span style='float: right;'>ARM</span>";
    } else if (isTar) {
      architecture = "ARM <span style='float: right;'>Binary</span>";
    } else {
      architecture = "ARM";
    }
  } else if (ARCH_PATTERNS.X86.some(p => name.includes(p))) {
    if (isAppImage) {
      architecture = "AppImage <span style='float: right;'>x86_64</span>";
    } else if (isDebian) {
      architecture = "Debian <span style='float: right;'>x86_64</span>";
    } else if (isTar) {
      architecture = "x86_64 <span style='float: right;'>Binary</span>";
    } else {
      architecture = "x86_64";
    }
  }

  // Determine type
  let type: DownloadAsset['type'] = "archive";
  if (name.includes(FILE_EXTENSIONS.EXE)) type = "executable";
  else if (name.includes(FILE_EXTENSIONS.MSI) || name.includes(FILE_EXTENSIONS.DEB) || name.includes(FILE_EXTENSIONS.RPM)) type = "installer";
  else if (name.includes(FILE_EXTENSIONS.DMG) || name.includes(FILE_EXTENSIONS.APP_BUNDLE)) type = "bundle";
  else if (name.includes(FILE_EXTENSIONS.APPIMAGE)) type = "appimage";

  return { platform, architecture, type };
}

/**
 * Get display name for download asset
 */
function getDisplayName(assetName: string, type: DownloadAsset['type']): string {
  const name = assetName.toLowerCase();

  // Be specific about file types
  if (name.includes(FILE_EXTENSIONS.DMG)) return 'DMG Installer';
  if (name.includes(FILE_EXTENSIONS.APPIMAGE)) return 'AppImage';
  if (name.includes(FILE_EXTENSIONS.DEB)) return 'DEB Package';
  if (name.includes(FILE_EXTENSIONS.RPM)) return 'RPM Package';
  if (name.includes(FILE_EXTENSIONS.MSI)) return 'MSI Installer';
  if (name.includes(FILE_EXTENSIONS.EXE)) return 'Executable';
  if (name.includes(FILE_EXTENSIONS.APP_BUNDLE)) return 'macOS App Bundle';
  if (name.includes(FILE_EXTENSIONS.TAR)) return 'TAR Archive';
  if (name.includes(FILE_EXTENSIONS.ZIP)) return 'ZIP Archive';

  // Fallback to generic types
  switch (type) {
    case 'executable': return 'Executable';
    case 'installer': return 'Installer';
    case 'bundle': return 'Bundle';
    case 'appimage': return 'AppImage';
    case 'archive': return 'Archive';
    default: return assetName;
  }
}

/**
 * Format file size from bytes to human readable format
 */
function formatFileSize(bytes: number): string {
  let size = bytes;
  let unitIndex = 0;

  while (size >= 1024 && unitIndex < FILE_SIZE_UNITS.length - 1) {
    size /= 1024;
    unitIndex++;
  }

  return `${Math.round(size)} ${FILE_SIZE_UNITS[unitIndex]}`;
}



/**
 * Generate HTML table for GUI downloads
 */
export function generateGuiTable(releaseInfo: ReleaseInfo): string {
  // Filter for GUI assets (eigenwallet_*)
  const guiAssets = releaseInfo.assets.filter(asset =>
    asset.downloadUrl.includes(ASSET_PREFIXES.GUI)
  );

  // Add hard-coded Flatpak row for Linux
  const flatpakAsset: DownloadAsset = {
    name: "Flatpak",
    downloadUrl: "/flatpak.html",
    signatureUrl: "",
    size: "",
    architecture: "x86_64 <span style='float: right;'>Flatpak</span>",
    platform: "Linux",
    type: "instructions"
  };
  const aurAsset: DownloadAsset = {
    name: "AUR",
    downloadUrl: "/download.html#aur",
    signatureUrl: "",
    size: "",
    architecture: "x86_64 <span style='float: right;'>AUR</span>",
    platform: "Linux",
    type: "instructions"
  };

  const linuxNotice = "Due to a WebKit bug our AppImage/DEB may not work on older OSes. Try the Flatpak if you're having trouble on Linux.";

  return generateTable([...guiAssets, flatpakAsset, aurAsset], "GUI Downloads", releaseInfo.releaseDate, { Linux: linuxNotice });
}

/**
 * Generate HTML table for CLI downloads
 */
export function generateCliTable(releaseInfo: ReleaseInfo): string {
  // Filter for CLI assets (asb_*, swap_*, orchestrator_*, and rendezvous-server_*)
  const cliAssets = releaseInfo.assets.filter(asset =>
    asset.downloadUrl.includes(ASSET_PREFIXES.CLI_ASB) ||
    asset.downloadUrl.includes(ASSET_PREFIXES.CLI_ASB_CONTROLLER) ||
    asset.downloadUrl.includes(ASSET_PREFIXES.CLI_SWAP) ||
    asset.downloadUrl.includes(ASSET_PREFIXES.CLI_ORCHESTRATOR) ||
    asset.downloadUrl.includes(ASSET_PREFIXES.CLI_RENDEZVOUS)
  );
  const aurAsset: DownloadAsset = {
    name: "AUR",
    downloadUrl: "/download.html#aur",
    signatureUrl: "",
    size: "",
    architecture: "x86_64 <span style='float: right;'>AUR</span>",
    platform: "Linux",
    type: "instructions"
  };

  return generateTable([...cliAssets, aurAsset], "CLI Tools");
}

/**
 * Fetch AUR package version from AUR API
 */
async function fetchAurPackageVersion(packageName: string): Promise<string> {
  try {
    const response = await fetch(`https://aur.archlinux.org/rpc/v5/info?arg[]=${packageName}`);
    if (!response.ok) {
      console.warn(`AUR API responded with status: ${response.status} for package: ${packageName}`);
      return 'N/A';
    }

    const data = await response.json() as any;
    if (data.results && data.results.length > 0) {
      return data.results[0].Version || 'N/A';
    }
    return 'N/A';
  } catch (error) {
    console.warn(`Failed to fetch AUR version for ${packageName}:`, error);
    return 'N/A';
  }
}

/**
 * Generate HTML table for AUR packages
 */
export async function generateAurTable(): Promise<string> {
  // Fetch versions for all packages in parallel
  const packagesWithVersions = await Promise.all(
    AUR_PACKAGES.map(async (pkg) => {
      const version = await fetchAurPackageVersion(pkg.name);
      return { ...pkg, version };
    })
  );

  const tableRows = packagesWithVersions.map(pkg => {
    return `  <tr>
    <td class="hide-mobile">${pkg.architectures.join(", ")}</td>
    <td><a href="${pkg.packageUrl}"><code>${pkg.name}</code></a></td>
    <td>${pkg.version}</td>
    <td><a href="${pkg.maintainerUrl}">${pkg.maintainer}</a></td>
  </tr>`;
  }).join('\n');

  return `<table>
  <thead>
    <tr>
      <th class="hide-mobile">Architecture</th>
      <th>Package</th>
      <th>Version</th>
      <th>Maintainer</th>
    </tr>
  </thead>
  <tbody>
${tableRows}
  <td colspan="4" class="notice">
  The Arch packages are unofficial and communitity maintained. Use at your own risk.
  </td>
  </tbody>
</table>`;
}

/**
 * Check if a release is less than 36 hours old
 */
function isRecentRelease(releaseDate: string): boolean {
  const releaseTime = new Date(releaseDate).getTime();
  const now = Date.now();
  const twentyFourHoursInMs = 36 * 60 * 60 * 1000;
  return (now - releaseTime) < twentyFourHoursInMs;
}

/**
 * Check if GUI builds are missing for any major platform
 */
function hasGuiBuildsForAllPlatforms(assets: DownloadAsset[]): { hasLinux: boolean; hasWindows: boolean; hasMacOS: boolean } {
  const platforms = assets.map(asset => asset.platform);
  return {
    hasLinux: platforms.includes('Linux'),
    hasWindows: platforms.includes('Windows'),
    hasMacOS: platforms.includes('macOS')
  };
}

/**
 * Generate HTML table for downloads
 */
function generateTable(assets: DownloadAsset[], title: string, releaseDate?: string, platformNotices?: Record<string, string>): string {
  if (assets.length === 0) {
    return `<p><em>No ${title.toLowerCase()} available for this release.</em></p>`;
  }

  // Group assets by platform
  const platformGroups = assets.reduce((groups, asset) => {
    if (!groups[asset.platform]) {
      groups[asset.platform] = [];
    }
    groups[asset.platform].push(asset);
    return groups;
  }, {} as Record<string, DownloadAsset[]>);

  let tableHtml = `
<table>
  <thead>
    <tr>
      <th scope="col">Architecture</th>
      <th scope="col">File</th>
      <th scope="col">Signature</th>
      <th scope="col">Size</th>
    </tr>
  </thead>
  <tbody>`;

  // Check if this is a GUI table and if any major platform builds are missing
  const isGuiTable = title === "GUI Downloads";
  let showRecentReleaseNotice = false;
  
  if (isGuiTable && releaseDate) {
    const platformStatus = hasGuiBuildsForAllPlatforms(assets.filter(asset => asset.type !== 'instructions'));
    const isRecent = isRecentRelease(releaseDate);
    const hasMissingBuilds = !platformStatus.hasLinux || !platformStatus.hasWindows || !platformStatus.hasMacOS;
    
    showRecentReleaseNotice = isRecent && hasMissingBuilds;
  }

  // Define platform order and icons
  for (const platform of PLATFORM_ORDER) {
    if (!platformGroups[platform]) continue;

    const platformAssets = platformGroups[platform];
    const icon = PLATFORM_ICONS[platform] || "";

    // Add platform header row
    tableHtml += `
    <tr>
      <td colspan="4" style="background-color: var(--pre-bg-color); font-weight: bold; padding: 0.75rem;">
        ${icon} ${platform}
      </td>
    </tr>`;

    // Add asset rows
    for (const asset of platformAssets) {
      const architecture = asset.architecture;
      const fileName = asset.downloadUrl.split('/').pop() || 'Unknown';
      const downloadIcon = asset.downloadUrl.includes('github.com/eigenwallet/core/releases/download')
        ? `<img src="/imgs/download.svg" alt="download" style="height: 1em; width: 1em; vertical-align: baseline; display: inline; margin-left: 0.25em; filter: invert(25%) sepia(85%) saturate(4500%) hue-rotate(355deg) brightness(95%) contrast(95%);">`
        : '';
      const fileNameLink = asset.type === 'instructions'
        ? `<a href="${asset.downloadUrl}">Instructions</a>`
        : `<a href="${asset.downloadUrl}"><code style="font-size: 0.85em; word-break: break-all;">${fileName}</code>${downloadIcon}</a>`;
      const signatureLink = asset.signatureUrl
        ? `<a href="${asset.signatureUrl}">signature</a>`
        : '';

      tableHtml += `
    <tr>
      <td>${architecture}</td>
      <td>${fileNameLink}</td>
      <td>${signatureLink}</td>
      <td>${asset.size}</td>
    </tr>`;
    }

    // Add platform-specific notice if provided
    if (platformNotices && platformNotices[platform]) {
      tableHtml += `
    <tr>
      <td colspan="4" class="info">
        ${platformNotices[platform]}
      </td>
    </tr>`;
    }
  }

  // Add notice for recent releases with missing GUI builds
  if (showRecentReleaseNotice) {
    tableHtml += `
    <tr>
      <td colspan="4" class="notice">
        Binaries are still being built for this release... Please check back in a few hours
      </td>
    </tr>`;
  }

  tableHtml += `
  </tbody>
</table>`;

  return tableHtml;
}

/**
 * Download an asset file to local assets directory
 */
async function downloadAsset(url: string, filename: string): Promise<string> {
  const assetsDir = path.join(process.cwd(), 'dist', 'assets');

  // Ensure assets directory exists
  if (!fs.existsSync(assetsDir)) {
    fs.mkdirSync(assetsDir, { recursive: true });
  }

  const localPath = path.join(assetsDir, filename);
  const relativePath = `assets/${filename}`;

  // Check if file already exists
  if (fs.existsSync(localPath)) {
    console.log(`Asset already exists: ${relativePath}`);
    return relativePath;
  }

  try {
    console.log(`\nDownloading ${filename}...`);
    const response = await fetch(url);

    if (!response.ok) {
      console.warn(`Failed to download ${filename}: ${response.status}`);
      return url; // Fallback to original URL
    }

    const contentLength = parseInt(response.headers.get('content-length') || '0');
    const reader = response.body?.getReader();

    if (!reader) {
      console.warn(`No response body for ${filename}`);
      return url;
    }

    // Create progress bar
    const progressBar = new cliProgress.SingleBar({
      format: `  {filename} |{bar}| {percentage}% | {value}/{total} MB | ETA: {eta}s | Speed: {speed} MB/s`,
      barCompleteChar: '\u2588',
      barIncompleteChar: '\u2591',
      hideCursor: true
    }, cliProgress.Presets.shades_classic);

    const totalMB = Math.round((contentLength / 1024 / 1024) * 100) / 100;
    progressBar.start(totalMB, 0, {
      filename: filename.length > 25 ? '...' + filename.slice(-22) : filename,
      speed: '0.00'
    });

    const chunks: Uint8Array[] = [];
    let receivedLength = 0;
    const startTime = Date.now();

    while (true) {
      const { done, value } = await reader.read();

      if (done) break;

      chunks.push(value);
      receivedLength += value.length;

      // Update progress bar
      const receivedMB = Math.round((receivedLength / 1024 / 1024) * 100) / 100;
      const elapsedSeconds = (Date.now() - startTime) / 1000;
      const speed = elapsedSeconds > 0 ? Math.round((receivedMB / elapsedSeconds) * 100) / 100 : 0;

      progressBar.update(receivedMB, {
        filename: filename.length > 25 ? '...' + filename.slice(-22) : filename,
        speed: speed.toFixed(2)
      });
    }

    progressBar.stop();

    // Combine chunks and write to file
    const buffer = new Uint8Array(receivedLength);
    let offset = 0;
    for (const chunk of chunks) {
      buffer.set(chunk, offset);
      offset += chunk.length;
    }

    fs.writeFileSync(localPath, buffer);
    console.log(`✓ Downloaded ${filename} (${totalMB} MB)`);
    return relativePath;
  } catch (error) {
    console.warn(`Error downloading ${filename}:`, error);
    return url; // Fallback to original URL
  }
}

/**
 * Download all assets for a release and update URLs
 */
async function downloadReleaseAssets(releaseInfo: ReleaseInfo): Promise<ReleaseInfo> {
  const updatedAssets = await Promise.all(
    releaseInfo.assets.map(async (asset) => {
      const filename = asset.downloadUrl.split('/').pop() || 'unknown';
      const signatureFilename = `${filename}.asc`;

      // Download main asset
      const localDownloadUrl = await downloadAsset(asset.downloadUrl, filename);

      // Download signature file if it exists
      let localSignatureUrl = asset.signatureUrl;
      if (asset.signatureUrl) {
        localSignatureUrl = await downloadAsset(asset.signatureUrl, signatureFilename);
      }

      return {
        ...asset,
        downloadUrl: localDownloadUrl,
        signatureUrl: localSignatureUrl
      };
    })
  );

  return {
    ...releaseInfo,
    assets: updatedAssets
  };
}

/**
 * Process download template with actual data
 */
export async function processDownloadTemplate(template: string, downloadAssets: boolean = false): Promise<string> {
  let releaseInfo = await generateDownloadData();

  // Download assets locally if flag is enabled
  if (downloadAssets) {
    console.log('Downloading assets locally...');
    releaseInfo = await downloadReleaseAssets(releaseInfo);
  }

  const guiTable = generateGuiTable(releaseInfo);
  const cliTable = generateCliTable(releaseInfo);
  const aurTable = await generateAurTable();

  return template
    .replace(new RegExp(TEMPLATE_PLACEHOLDERS.VERSION.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g'), releaseInfo.version)
    .replace(new RegExp(TEMPLATE_PLACEHOLDERS.RELEASE_DATE.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g'), releaseInfo.releaseDate)
    .replace(new RegExp(TEMPLATE_PLACEHOLDERS.GUI_TABLE.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g'), guiTable)
    .replace(new RegExp(TEMPLATE_PLACEHOLDERS.CLI_TABLE.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g'), cliTable)
    .replace(new RegExp(TEMPLATE_PLACEHOLDERS.AUR_TABLE.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g'), aurTable)
    .replace(new RegExp(TEMPLATE_PLACEHOLDERS.HASHES_LINK.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g'), releaseInfo.hashesUrl)
    .replace(new RegExp(TEMPLATE_PLACEHOLDERS.SIGNING_KEY_LINK.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'g'), releaseInfo.signingKeyUrl);
}
