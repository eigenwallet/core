import { createSlice, PayloadAction } from "@reduxjs/toolkit";
import { Message } from "../../models/apiModel";

export interface ConversationsSlice {
  // List of feedback IDs we know of
  knownFeedbackIds: string[];
  // Maps feedback IDs to conversations using the updated Message type
  conversations: {
    [key: string]: Message[]; // Use the imported Message type
  };
  // Stores IDs for Messages that have been seen by the user
  seenMessages: string[];
}

const initialState: ConversationsSlice = {
  knownFeedbackIds: [],
  conversations: {},
  seenMessages: [],
};

const conversationsSlice = createSlice({
  name: "conversations",
  initialState,
  reducers: {
    addFeedbackId(slice, action: PayloadAction<string>) {
      // Only add if not already present
      if (!slice.knownFeedbackIds.includes(action.payload)) {
        slice.knownFeedbackIds.push(action.payload);
      }
    },
    // Sets the conversations for a given feedback id (Payload uses the correct Message type)
    setConversation(
      slice,
      action: PayloadAction<{ feedbackId: string; messages: Message[] }>,
    ) {
      slice.conversations[action.payload.feedbackId] = action.payload.messages;
    },
    // Sets the seen messages for a given feedback id (Payload uses the correct Message type)
    markMessagesAsSeen(slice, action: PayloadAction<Message[]>) {
      const newSeenIds = action.payload
        .map((msg) => msg.id.toString())
        .filter((id) => !slice.seenMessages.includes(id)); // Avoid duplicates
      slice.seenMessages.push(...newSeenIds);
    },
  },
});

export const { addFeedbackId, setConversation, markMessagesAsSeen } =
  conversationsSlice.actions;
export default conversationsSlice.reducer;
