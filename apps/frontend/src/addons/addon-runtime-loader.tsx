import { useEffect } from "react";
import { loadAllAddons } from "./addons-loader";

let hasStartedAddonRuntime = false;

export function AddonRuntimeLoader() {
  useEffect(() => {
    if (hasStartedAddonRuntime) {
      return;
    }

    hasStartedAddonRuntime = true;
    void loadAllAddons();
  }, []);

  return null;
}
