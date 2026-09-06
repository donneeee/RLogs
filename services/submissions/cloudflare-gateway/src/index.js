const PUBLIC_ROUTES = [
  ["GET", /^\/health$/],
  ["GET", /^\/v1\/auth\/config$/],
  ["GET", /^\/v1\/auth\/discord\/start$/],
  ["GET", /^\/v1\/auth\/discord\/callback$/],
  ["POST", /^\/v1\/auth\/discord\/complete$/],
  ["GET", /^\/v1\/auth\/me$/],
  ["PATCH", /^\/v1\/auth\/me$/],
  ["PATCH", /^\/v1\/auth\/me\/parse-publication$/],
  ["GET", /^\/v1\/auth\/device$/],
  ["GET", /^\/v1\/auth\/profiles$/],
  ["GET", /^\/v1\/auth\/parses$/],
  ["GET", /^\/v1\/auth\/parses\/[A-Za-z0-9_-]+$/],
  ["PATCH", /^\/v1\/auth\/parses\/[A-Za-z0-9_-]+\/visibility$/],
  ["GET", /^\/v1\/parses$/],
  ["GET", /^\/v1\/parses\/[A-Za-z0-9_-]+$/],
  ["GET", /^\/v1\/activity\/milestones$/],
  ["GET", /^\/v1\/run-groups\/[A-Za-z0-9_-]+\/reconciliation$/],
  ["GET", /^\/v1\/profiles$/],
  ["GET", /^\/v1\/photos$/],
  ["GET", /^\/v1\/profiles\/prf_[a-z0-9_]+$/],
  ["GET", /^\/v1\/profiles\/prf_[a-z0-9_]+\/loadouts\/[1-9][0-9]*$/],
  ["GET", /^\/v1\/profiles\/prf_[a-z0-9_]+\/photo-wall\/[1-9][0-9]*$/],
  ["GET", /^\/v1\/users\/[1-9][0-9]{11}$/],
  ["PUT", /^\/v1\/profiles\/prf_[a-z0-9_]+\/photo-wall\/[1-9][0-9]*\/like$/],
  ["DELETE", /^\/v1\/profiles\/prf_[a-z0-9_]+\/photo-wall\/[1-9][0-9]*\/like$/],
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
    "Access-Control-Allow-Methods": "DELETE, GET, PATCH, POST, PUT, OPTIONS",
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

function configuredOrigin(value, { allowDevelopmentOrigin = false } = {}) {
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
  const hostname = origin.hostname.toLowerCase();
  const developmentOrigin =
    hostname === "localhost" ||
    hostname === "127.0.0.1" ||
    hostname === "::1" ||
    hostname.endsWith(".trycloudflare.com");
  if (developmentOrigin && !allowDevelopmentOrigin) {
    throw new Error("ORIGIN_BASE_URL cannot target a workstation or quick tunnel in production");
  }
  return origin;
}

function unavailableResponse(cors) {
  const headers = new Headers(cors);
  headers.set("Cache-Control", "no-store");
  headers.set("Retry-After", "30");
  headers.set("X-Content-Type-Options", "nosniff");
  return Response.json(
    { error: "submission origin is unavailable", retryable: true },
    { status: 503, headers },
  );
}

async function serviceFailureResponse(upstream, cors) {
  let payload;
  try {
    payload = await upstream.json();
  } catch {
    return unavailableResponse(cors);
  }
  if (
    !payload ||
    typeof payload !== "object" ||
    typeof payload.error !== "string" ||
    payload.error.length === 0 ||
    payload.error.length > 200
  ) {
    return unavailableResponse(cors);
  }
  const headers = new Headers(cors);
  headers.set("Cache-Control", "no-store");
  headers.set("Retry-After", upstream.headers.get("Retry-After") ?? "30");
  headers.set("X-Content-Type-Options", "nosniff");
  return Response.json(
    { error: payload.error, retryable: payload.retryable === true },
    { status: upstream.status, headers },
  );
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

    let upstreamResponse;
    let usedServiceBinding = false;
    if (env.BACKEND && typeof env.BACKEND.fetch === "function") {
      try {
        upstreamResponse = await env.BACKEND.fetch(request);
        usedServiceBinding = true;
      } catch {
        return unavailableResponse(cors);
      }
    }
    let origin;
    if (!upstreamResponse) {
    try {
      origin = configuredOrigin(env.ORIGIN_BASE_URL, {
        allowDevelopmentOrigin: env.ALLOW_DEVELOPMENT_ORIGIN === "true",
      });
    } catch {
      return Response.json(
        { error: "submission origin is not configured" },
        { status: 503, headers: cors },
      );
    }
    const upstreamUrl = new URL(`${requestUrl.pathname}${requestUrl.search}`, origin);
    const upstreamRequest = new Request(upstreamUrl, request);
    try {
      upstreamResponse = await fetch(upstreamRequest);
    } catch {
      return unavailableResponse(cors);
    }
    }
    // Tunnel failures arrive as HTTP responses, not fetch exceptions. Never
    // expose their HTML or cache them as successful public photo responses.
    if (upstreamResponse.status >= 500) {
      if (usedServiceBinding) {
        return serviceFailureResponse(upstreamResponse, cors);
      }
      if (upstreamResponse.body) {
        await upstreamResponse.body.cancel().catch(() => {});
      }
      return unavailableResponse(cors);
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
    headers.set("Cache-Control", publicPhoto && upstreamResponse.ok ? "public, max-age=300" : "no-store");
    headers.set("X-Content-Type-Options", "nosniff");
    return new Response(upstreamResponse.body, {
      status: upstreamResponse.status,
      statusText: upstreamResponse.statusText,
      headers,
    });
  },
};

export { configuredOrigin, routeAllowed };
