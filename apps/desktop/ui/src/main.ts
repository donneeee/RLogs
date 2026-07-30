import "./styles/shell.css";

import { createDevelopmentAdapter } from "./adapters/development-adapter";
import { createLocalHostAdapterIfAvailable } from "./adapters/local-host-adapter";
import { DesktopShell } from "./shell/desktop-shell";

const root = document.querySelector<HTMLElement>("#app");
if (root === null) {
  throw new Error("rLogs desktop shell requires an #app element");
}

const adapter =
  (await createLocalHostAdapterIfAvailable()) ?? createDevelopmentAdapter();
const shell = new DesktopShell(root, adapter);
void shell.start();
