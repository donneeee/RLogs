import { readFile } from "node:fs/promises";

import { validatePackage } from "../src/profile.js";

const [packagePath] = process.argv.slice(2);
const deviceToken = process.env.RLOGS_PROFILE_VERIFICATION_TOKEN;
if (!packagePath || !deviceToken) {
  throw new Error("usage: RLOGS_PROFILE_VERIFICATION_TOKEN=<token> node scripts/verify-profile-package.mjs <current.profile.json>");
}
const packageValue = JSON.parse(await readFile(packagePath, "utf8"));
const deviceId = packageValue?.source?.live_capture?.device_id;
const result = await validatePackage(packageValue, deviceId, deviceToken);
if (result.error) throw new Error(result.error);
process.stdout.write(`verified ${packageValue.package_id}\n`);
