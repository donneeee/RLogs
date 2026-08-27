export interface CaptureInterface {
  value: string;
  label: string;
  friendly_name: string | null;
  description: string | null;
  mac_address: string | null;
  is_up: boolean | null;
  is_virtual: boolean | null;
  recommendation: "game_traffic" | "system_route" | null;
}

export interface CaptureEnvironment {
  capture_interfaces: CaptureInterface[];
  recommended_capture_interface: string | null;
  recommended_capture_source: "game_traffic" | "system_route" | null;
  recommended_capture_reason: string | null;
}

export interface CaptureInterfaceSelection {
  device: CaptureInterface | null;
  source: "game_traffic" | "saved" | "system_route" | "fallback" | "none";
  replacedSavedDevice: boolean;
}

/**
 * A direct game-socket match is authoritative. A manually saved, active
 * adapter wins over the weaker system-route fallback. Missing/disconnected
 * saved devices are never selected silently.
 */
export function selectCaptureInterface(
  environment: CaptureEnvironment,
  savedValue: string | null,
): CaptureInterfaceSelection {
  const devices = environment.capture_interfaces;
  const saved = devices.find((device) => device.value === savedValue);
  const recommended = devices.find(
    (device) => device.value === environment.recommended_capture_interface,
  );
  const savedIsUsable = saved !== undefined && saved.is_up !== false;
  const replacedSavedDevice =
    savedValue !== null &&
    (saved === undefined ||
      saved.is_up === false ||
      (environment.recommended_capture_source === "game_traffic" &&
        recommended !== undefined &&
        saved.value !== recommended.value));

  if (
    environment.recommended_capture_source === "game_traffic" &&
    recommended !== undefined
  ) {
    return {
      device: recommended,
      source: "game_traffic",
      replacedSavedDevice,
    };
  }
  if (savedIsUsable) {
    return {
      device: saved,
      source: "saved",
      replacedSavedDevice: false,
    };
  }
  if (recommended !== undefined) {
    return {
      device: recommended,
      source: "system_route",
      replacedSavedDevice,
    };
  }
  const fallback =
    devices.find(
      (device) => device.is_up === true && device.is_virtual !== true,
    ) ??
    devices.find((device) => device.is_up === true) ??
    devices[0] ??
    null;
  return {
    device: fallback,
    source: fallback === null ? "none" : "fallback",
    replacedSavedDevice,
  };
}

export function captureInterfaceSummary(
  selection: CaptureInterfaceSelection,
  environment: CaptureEnvironment,
  savedValue: string | null,
): string {
  const device = selection.device;
  if (device === null) {
    return "No capture interface is available.";
  }
  const name =
    device.friendly_name ?? device.description ?? `device ${device.value}`;
  if (selection.source === "game_traffic") {
    return (
      environment.recommended_capture_reason ??
      `${name} was matched directly to active game traffic.`
    );
  }
  if (selection.replacedSavedDevice && savedValue !== null) {
    return `Saved device ${savedValue} is disconnected or unavailable; selected ${name}.`;
  }
  if (selection.source === "system_route") {
    return (
      environment.recommended_capture_reason ??
      `${name} is the active routed network adapter.`
    );
  }
  if (selection.source === "saved") {
    return `Using saved device ${name}.`;
  }
  return `Using detected device ${name}; verify it before starting capture.`;
}
