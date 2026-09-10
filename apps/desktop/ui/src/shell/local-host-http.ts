export class LocalHostHttpError extends Error {
  constructor(public readonly status: number, message: string) {
    super(message);
    this.name = "LocalHostHttpError";
  }
}

export async function localHostJson(response: Response, fallback: string): Promise<unknown> {
  const body: unknown = await response.json().catch(() => null);
  if (!response.ok) {
    const detail = typeof body === "object" && body !== null && "error" in body && typeof body.error === "string"
      ? body.error : fallback;
    throw new LocalHostHttpError(response.status, detail);
  }
  return body;
}
