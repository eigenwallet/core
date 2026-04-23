// This file is responsible for making HTTP requests to the Unstoppable API and to the CoinGecko API.
// The APIs are used to:
// - fetch provider status from the public registry
// - fetch alerts to be displayed to the user
// - and to submit feedback
// - fetch currency rates from CoinGecko

import { invoke as invokeUnsafe } from "@tauri-apps/api/core";
import { Alert, AttachmentInput, Message } from "models/apiModel";
import { store } from "./store/storeRenderer";
import {
  setBtcPrice,
  setXmrBtcRate,
  setXmrPrice,
} from "store/features/ratesSlice";
import { FiatCurrency } from "store/features/settingsSlice";
import { setAlerts } from "store/features/alertsSlice";
import logger from "utils/logger";
import { setConversation } from "store/features/conversationsSlice";

const PUBLIC_REGISTRY_API_BASE_URL = "https://api.unstoppableswap.net";

interface HttpResponse {
  status: number;
  body: string;
}

async function httpGet(url: string): Promise<HttpResponse> {
  return invokeUnsafe("http_get", {
    args: { url },
  }) as Promise<HttpResponse>;
}

async function httpPostJson(
  url: string,
  body: unknown,
): Promise<HttpResponse> {
  return invokeUnsafe("http_post_json", {
    args: { url, body: JSON.stringify(body) },
  }) as Promise<HttpResponse>;
}

function ensureSuccessfulResponse(response: HttpResponse, url: string): void {
  if (response.status >= 200 && response.status < 300) {
    return;
  }

  throw new Error(
    `Request to ${url} failed. Status: ${response.status}. Body: ${response.body}`,
  );
}

function parseJsonResponse<T>(response: HttpResponse, url: string): T {
  ensureSuccessfulResponse(response, url);
  return JSON.parse(response.body) as T;
}

async function fetchAlertsViaHttp(): Promise<Alert[]> {
  const url = `${PUBLIC_REGISTRY_API_BASE_URL}/api/alerts`;
  const response = await httpGet(url);
  return parseJsonResponse<Alert[]>(response, url);
}

export async function submitFeedbackViaHttp(
  content: string,
  attachments?: AttachmentInput[],
): Promise<string> {
  type Response = string;

  const requestPayload = {
    content,
    attachments: attachments || [], // Ensure attachments is always an array
  };

  const url = `${PUBLIC_REGISTRY_API_BASE_URL}/api/submit-feedback`;
  const response = await httpPostJson(
    url,
    requestPayload, // Send the corrected structure
  );
  return parseJsonResponse<Response>(response, url);
}

export async function fetchFeedbackMessagesViaHttp(
  feedbackId: string,
): Promise<Message[]> {
  const url = `${PUBLIC_REGISTRY_API_BASE_URL}/api/feedback/${feedbackId}/messages`;
  const response = await httpGet(url);
  return parseJsonResponse<Message[]>(response, url);
}

export async function appendFeedbackMessageViaHttp(
  feedbackId: string,
  content: string,
  attachments?: AttachmentInput[],
): Promise<number> {
  type Response = number;

  const body = {
    feedback_id: feedbackId,
    content,
    attachments: attachments || [], // Ensure attachments is always an array
  };

  const url = `${PUBLIC_REGISTRY_API_BASE_URL}/api/append-feedback-message`;
  const response = await httpPostJson(
    url,
    body, // Send new structure
  );
  return parseJsonResponse<Response>(response, url);
}

async function fetchCurrencyPrice(
  currency: string,
  fiatCurrency: FiatCurrency,
): Promise<number> {
  const url = `https://api.coingecko.com/api/v3/simple/price?ids=${currency}&vs_currencies=${fiatCurrency.toLowerCase()}`;
  const response = await httpGet(url);
  const data = parseJsonResponse<Record<string, Record<string, number>>>(
    response,
    url,
  );
  return data[currency][fiatCurrency.toLowerCase()];
}

async function fetchXmrBtcRate(): Promise<number> {
  const url = "https://api.kraken.com/0/public/Ticker?pair=XMRXBT";
  const response = await httpGet(url);
  const data = parseJsonResponse<{
    error?: string[];
    result: {
      XXMRXXBT: {
        c: [string, string];
      };
    };
  }>(response, url);

  if (data.error && data.error.length > 0) {
    throw new Error(`Kraken API error: ${data.error[0]}`);
  }

  const result = data.result.XXMRXXBT;
  const lastTradePrice = parseFloat(result.c[0]);

  return lastTradePrice;
}

function fetchBtcPrice(fiatCurrency: FiatCurrency): Promise<number> {
  return fetchCurrencyPrice("bitcoin", fiatCurrency);
}

async function fetchXmrPrice(fiatCurrency: FiatCurrency): Promise<number> {
  return fetchCurrencyPrice("monero", fiatCurrency);
}

/**
 * If enabled by the user, fetch the XMR, BTC and XMR/BTC rates
 * and store them in the Redux store.
 */
export async function updateRates(): Promise<void> {
  const settings = store.getState().settings;
  if (!settings.fetchFiatPrices) return;

  try {
    const xmrBtcRate = await fetchXmrBtcRate();
    store.dispatch(setXmrBtcRate(xmrBtcRate));
  } catch (error) {
    logger.error(error, "Error fetching XMR/BTC market rate");
  }

  try {
    const btcPrice = await fetchBtcPrice(settings.fiatCurrency);
    store.dispatch(setBtcPrice(btcPrice));
  } catch (error) {
    logger.error(error, `Error fetching BTC price in ${settings.fiatCurrency}`);
  }

  try {
    const xmrPrice = await fetchXmrPrice(settings.fiatCurrency);
    store.dispatch(setXmrPrice(xmrPrice));
  } catch (error) {
    logger.error(error, `Error fetching XMR price in ${settings.fiatCurrency}`);
  }

  logger.info(`Finished rate update for ${settings.fiatCurrency}`);
}

/**
 * Fetch all alerts
 */
export async function updateAlerts(): Promise<void> {
  try {
    const alerts = await fetchAlertsViaHttp();
    store.dispatch(setAlerts(alerts));
  } catch (error) {
    logger.error(error, "Error fetching alerts");
  }
}

/**
 * Fetch all conversations
 * Goes through all feedback ids and fetches all the messages for each feedback id
 */
export async function fetchAllConversations(): Promise<void> {
  const feedbackIds = store.getState().conversations.knownFeedbackIds;

  console.log("Fetching all conversations", feedbackIds);

  for (const feedbackId of feedbackIds) {
    try {
      console.log("Fetching messages for feedback id", feedbackId);
      const messages = await fetchFeedbackMessagesViaHttp(feedbackId);
      console.log("Fetched messages for feedback id", feedbackId, messages);
      store.dispatch(setConversation({ feedbackId, messages }));
    } catch (error) {
      logger.error(
        { error, feedbackId },
        "Error fetching messages for feedback",
      );
    }
  }
}
