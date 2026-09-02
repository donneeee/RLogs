const PUBLIC_ROUTES = [
  ["GET", /^\/health$/],
  ["GET", /^\/v1\/auth\/config$/],
  ["GET", /^\/v1\/auth\/discord\/start$/],
  ["GET", /^\/v1\/auth\/discord\/callback$/],
  ["POST", /^\/v1\/auth\/discord\/complete$/],
  ["GET", /^\/v1\/auth\/me$/],
  ["GET", /^\/v1\/auth\/device$/],
  ["GET", /^\/v1\/auth\/profiles$/],
  ["GET", /^\/v1\/auth\/parses$/],
  ["GET", /^\/v1\/auth\/parses\/[A-Za-z0-9_-]+$/],
  ["PATCH", /^\/v1\/auth\/parses\/[A-Za-z0-9_-]+\/visibility$/],
  ["GET", /^\/v1\/parses$/],
  ["GET", /^\/v1\/parses\/[A-Za-z0-9_-]+$/],
  ["GET", /^\/v1\/run-groups\/[A-Za-z0-9_-]+\/reconciliation$/],
  ["GET", /^\/v1\/profiles$/],
  ["GET", /^\/v1\/profiles\/prf_[a-z0-9_]+$/],
  ["GET", /^\/v1\/profiles\/prf_[a-z0-9_]+\/photo-wall\/[1-9][0-9]*$/],
];

const INGEST_ROUTES = [
  ["POST", /^\/v1\/uploads$/],
  ["POST", /^\/v1\/auth\/session\/exchange$/],
  ["POST", /^\/v1\/auth\/app-tokens$/],
  ["PUT", /^\/v1\/uploads\/[A-Za-z0-9_-]+\/chunks\/[0-9]+$/],
  ["POST", /^\/v1\/uploads\/[A-Za-z0-9_-]+\/finalize$/],
  ["POST", /^\/v1\/games\/blue-protocol-star-resonance\/profiles$/],
  [
    "PUT",
    /^\/v1\/games\/blue-protocol-star-resonance\/profiles\/prf_[a-z0-9_]+\/photo-wall\/[1-9][0-9]*$/,
  ],
];

function corsHeaders(origin, allowedOrigin) {
  const headers = new Headers({
    "Access-Control-Allow-Headers": "Authorization, Content-Type",
    "Access-Control-Allow-Methods": "GET, PATCH, POST, PUT, OPTIONS",
    "Access-Control-Max-Age": "86400",
    Vary: "Origin",
  });
  if (origin === allowedOrigin) {
    headers.set("Access-Control-Allow-Origin", origin);
  }
  return headers;
}

function routeAllowed(method, pathname) {
  return [...PUBLIC_ROUTES, ...INGEST_ROUTES].some(
    ([allowedMethod, pattern]) => allowedMethod === method && pattern.test(pathname),
  );
}

function configuredOrigin(value) {
  const origin = new URL(String(value ?? ""));
  if (
    origin.protocol !== "https:" ||
    origin.username ||
    origin.password ||
    origin.pathname !== "/" ||
    origin.search ||
    origin.hash
  ) {
    throw new Error("ORIGIN_BASE_URL must be an HTTPS origin without credentials or a path");
  }
  return origin;
}

export default {
  async fetch(request, env) {
    const requestUrl = new URL(request.url);
    const allowedOrigin = String(env.ALLOWED_ORIGIN ?? "");
    const requestOrigin = request.headers.get("Origin") ?? "";
    const cors = corsHeaders(requestOrigin, allowedOrigin);

    if (request.method === "OPTIONS") {
      if (requestOrigin !== allowedOrigin) {
        return new Response(null, { status: 403, headers: cors });
      }
      return new Response(null, { status: 204, headers: cors });
    }
    if (!routeAllowed(request.method, requestUrl.pathname)) {
      return Response.json({ error: "route not found" }, { status: 404, headers: cors });
    }

    let origin;
    try {
      origin = configuredOrigin(env.ORIGIN_BASE_URL);
    } catch {
      return Response.json(
        { error: "submission origin is not configured" },
        { status: 503, headers: cors },
      );
    }
    const upstreamUrl = new URL(`${requestUrl.pathname}${requestUrl.search}`, origin);
    const upstreamRequest = new Request(upstreamUrl, request);
    let upstreamResponse;
    try {
      upstreamResponse = await fetch(upstreamRequest);
    } catch {
      return Response.json(
        { error: "submission origin is unavailable" },
        { status: 502, headers: cors },
      );
    }
    const headers = new Headers(upstreamResponse.headers);
    for (const name of [
      "Access-Control-Allow-Credentials",
      "Access-Control-Allow-Headers",
      "Access-Control-Allow-Methods",
      "Access-Control-Allow-Origin",
      "Access-Control-Expose-Headers",
      "Access-Control-Max-Age",
    ]) {
      headers.delete(name);
    }
    for (const [name, value] of cors) {
      headers.set(name, value);
    }
    headers.delete("Set-Cookie");
    const publicPhoto =
      request.method === "GET" &&
      /^\/v1\/profiles\/prf_[a-z0-9_]+\/photo-wall\/[1-9][0-9]*$/.test(requestUrl.pathname);
    headers.set("Cache-Control", publicPhoto ? "public, max-age=300" : "no-store");
    headers.set("X-Content-Type-Options", "nosniff");
    return new Response(upstreamResponse.body, {
      status: upstreamResponse.status,
      statusText: upstreamResponse.statusText,
      headers,
    });
  },
};

export { configuredOrigin, routeAllowed };
