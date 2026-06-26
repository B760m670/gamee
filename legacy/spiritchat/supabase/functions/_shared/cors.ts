// Shared CORS headers for the Edge Function. Mobile (Expo) and web both call
// these endpoints cross-origin, so we allow any origin.
export const cors = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
  "Access-Control-Allow-Methods": "GET, POST, PUT, DELETE, OPTIONS",
}
