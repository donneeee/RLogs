import { describe, expect, it } from "vitest";

import {
  type CaptureEnvironment,
  selectCaptureInterface,
} from "./capture-interface";

const environment: CaptureEnvironment = {
  capture_interfaces: [
    {
      value: "8",
      label: "8 — Ethernet [Recommended: BPSR traffic, active]",
      friendly_name: "Ethernet",
      description: "Intel I211",
      mac_address: "00:11:22:33:44:55",
      is_up: true,
      is_virtual: false,
      recommendation: "game_traffic",
    },
    {
      value: "11",
      label: "11 — Local Area Connection [disconnected, virtual]",
      friendly_name: "Local Area Connection",
      description: "Speedify Virtual Adapter",
      mac_address: null,
      is_up: false,
      is_virtual: true,
      recommendation: null,
    },
  ],
  recommended_capture_interface: "8",
  recommended_capture_source: "game_traffic",
  recommended_capture_reason: "Ethernet carries BPSR traffic.",
};

describe("capture interface selection", () => {
  it("replaces a disconnected saved virtual adapter with the game match", () => {
    expect(selectCaptureInterface(environment, "11")).toMatchObject({
      device: { value: "8" },
      source: "game_traffic",
      replacedSavedDevice: true,
    });
  });

  it("treats a direct game match as stronger than a different active saved device", () => {
    const withActiveSaved: CaptureEnvironment = {
      ...environment,
      capture_interfaces: [
        environment.capture_interfaces[0]!,
        {
          ...environment.capture_interfaces[1]!,
          is_up: true,
        },
      ],
    };
    expect(selectCaptureInterface(withActiveSaved, "11")).toMatchObject({
      device: { value: "8" },
      source: "game_traffic",
      replacedSavedDevice: true,
    });
  });

  it("preserves an active saved device when only a route fallback exists", () => {
    const routeOnly: CaptureEnvironment = {
      ...environment,
      recommended_capture_source: "system_route",
    };
    const devices = routeOnly.capture_interfaces.map((device) => ({
      ...device,
      is_up: true,
    }));
    expect(
      selectCaptureInterface({ ...routeOnly, capture_interfaces: devices }, "11"),
    ).toMatchObject({
      device: { value: "11" },
      source: "saved",
      replacedSavedDevice: false,
    });
  });
});
