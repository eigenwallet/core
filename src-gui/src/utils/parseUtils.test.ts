import { describe, expect, it } from "vitest";
import { isValidMultiAddressWithPeerId } from "./parseUtils";

describe("isValidMultiAddressWithPeerId", () => {
  it("accepts a circuit relay address with relay and destination peer IDs", () => {
    const address =
      "/dns4/relay.example/tcp/443/wss/p2p/12D3KooWGRvf7qVQDrNR5nfYD6rKrbgeTi9x8RrbdxbmsPvxL4mw/p2p-circuit/p2p/12D3KooWMc39w7bZz4RLmJKuUiK9YkbKoEHACZWcL71XNns5dPuD";

    expect(isValidMultiAddressWithPeerId(address)).toBe(true);
  });

  it("rejects a circuit relay address without a destination peer ID", () => {
    const address =
      "/dns4/relay.example/tcp/443/wss/p2p/12D3KooWGRvf7qVQDrNR5nfYD6rKrbgeTi9x8RrbdxbmsPvxL4mw/p2p-circuit";

    expect(isValidMultiAddressWithPeerId(address)).toBe(false);
  });
});
