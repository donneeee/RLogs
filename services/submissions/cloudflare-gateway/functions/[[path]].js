import gateway from "../src/index.js";

export function onRequest(context) {
  return gateway.fetch(context.request, context.env, context);
}
