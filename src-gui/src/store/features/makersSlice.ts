import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { ExtendedMakerStatus, MakerStatus } from "models/apiModel";
import { SellerStatus } from "models/tauriModel";
import { getStubTestnetMaker } from "store/config";
import { rendezvousSellerToMakerStatus } from "utils/conversionUtils";
import { isMakerOutdated } from "utils/multiAddrUtils";

const stubTestnetMaker = getStubTestnetMaker();

export interface MakersSlice {
  rendezvous: {
    makers: (ExtendedMakerStatus | MakerStatus)[];
  };
  registry: {
    makers: ExtendedMakerStatus[] | null;
    // This counts how many failed connections attempts we have counted since the last successful connection
    connectionFailsCount: number;
  };
  selectedMaker: ExtendedMakerStatus | null;
}

const initialState: MakersSlice = {
  rendezvous: {
    makers: [],
  },
  registry: {
    makers: stubTestnetMaker ? [stubTestnetMaker] : null,
    connectionFailsCount: 0,
  },
  selectedMaker: null,
};

export const makersSlice = createSlice({
  name: "providers",
  initialState,
  reducers: {
    discoveredMakersByRendezvous(slice, action: PayloadAction<SellerStatus[]>) {
      action.payload.forEach((discoveredSeller) => {
        const discoveredMakerStatus =
          rendezvousSellerToMakerStatus(discoveredSeller);

        // If the seller has a status of "Unreachable" the provider is not added to the list
        if (discoveredMakerStatus === null) {
          return;
        }

        // If the provider was already discovered via the public registry, don't add it again
        const indexOfExistingMaker = slice.rendezvous.makers.findIndex(
          (prov) =>
            prov.peerId === discoveredMakerStatus.peerId &&
            prov.multiAddr === discoveredMakerStatus.multiAddr,
        );

        // Avoid duplicate entries, replace them instead
        if (indexOfExistingMaker !== -1) {
          slice.rendezvous.makers[indexOfExistingMaker] = discoveredMakerStatus;
        } else {
          slice.rendezvous.makers.push(discoveredMakerStatus);
        }
      });
    },
    setRegistryMakers(slice, action: PayloadAction<ExtendedMakerStatus[]>) {
      if (stubTestnetMaker) {
        action.payload.push(stubTestnetMaker);
      }
    },
    registryConnectionFailed(slice) {
      slice.registry.connectionFailsCount += 1;
    },
  },
});

export const {
  discoveredMakersByRendezvous,
  setRegistryMakers,
  registryConnectionFailed,
} = makersSlice.actions;

export default makersSlice.reducer;
