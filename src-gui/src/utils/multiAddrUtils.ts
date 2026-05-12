import { ExtendedMakerStatus, Maker } from "models/apiModel";
import semver from "semver";
import { isTestnet } from "store/config";

// const MIN_ASB_VERSION = "1.0.0-alpha.1" // First version to support new libp2p protocol
// const MIN_ASB_VERSION = "1.1.0-rc.3" // First version with support for bdk > 1.0
// const MIN_ASB_VERSION = "2.0.0-beta.1"; // First version with support for tx_early_refund
// const MIN_ASB_VERSION = "3.2.0-rc.1";
const VERSION_FLOOR_SOFT = "4.0.0"; // First version with partial refund path - completely incompatible
const VERSION_FLOOR_HARD = "4.0.0";

export function isMakerOnCorrectNetwork(
  provider: ExtendedMakerStatus,
): boolean {
  return provider.testnet === isTestnet();
}

/** Check whether a maker version is old and might not support newer features. */
export function isMakerVersionOld(version: string | undefined): boolean {
  if (version === undefined) return false;
  // This checks if the version is less than the minimum version
  // we use .compare(...) instead of .satisfies(...) because satisfies(...)
  // does not work with pre-release versions
  return semver.compare(version, VERSION_FLOOR_SOFT) === -1;
}

/** Check whether a maker version is too old and is known to be completely incompatile. */
export function isMakerVersionTooOld(version: string | undefined): boolean {
  if (version === undefined) return false;
  return semver.compare(version, VERSION_FLOOR_HARD) === -1;
}
