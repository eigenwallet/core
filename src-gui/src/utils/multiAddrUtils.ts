import { ExtendedMakerStatus, Maker } from "models/apiModel";
import { Multiaddr } from "multiaddr";
import semver from "semver";
import { isTestnet } from "store/config";

// const MIN_ASB_VERSION = "1.0.0-alpha.1" // First version to support new libp2p protocol
// const MIN_ASB_VERSION = "1.1.0-rc.3" // First version with support for bdk > 1.0
// const MIN_ASB_VERSION = "2.0.0-beta.1"; // First version with support for tx_early_refund
const MIN_ASB_VERSION = "3.2.0-rc.1";

export function isMakerOnCorrectNetwork(
  provider: ExtendedMakerStatus,
): boolean {
  return provider.testnet === isTestnet();
}

export function isMakerVersionOutdated(version: string): boolean {
  // This checks if the version is less than the minimum version
  // we use .compare(...) instead of .satisfies(...) because satisfies(...)
  // does not work with pre-release versions
  return semver.compare(version, MIN_ASB_VERSION) === -1;
}
