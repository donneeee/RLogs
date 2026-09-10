import { describe, expect, it } from "vitest";
import { LocalHostHttpError, localHostJson } from "./local-host-http";

describe("local host HTTP errors", () => {
  it("preserves status and host detail for callers that rebase only conflicts", async () => {
    const failure = localHostJson(new Response(JSON.stringify({ error: "revision changed" }), {
      status: 409, headers: { "Content-Type": "application/json" },
    }), "fallback");
    await expect(failure).rejects.toEqual(expect.objectContaining<Partial<LocalHostHttpError>>({
      name: "LocalHostHttpError", status: 409, message: "revision changed",
    }));
  });

  it("does not turn validation or storage failures into revision conflicts", async () => {
    for (const status of [422, 500]) {
      await expect(localHostJson(new Response("{}", { status }), "request failed"))
        .rejects.toEqual(expect.objectContaining({ status, message: "request failed" }));
    }
  });
});
