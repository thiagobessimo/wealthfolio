import { useEffect, useRef } from "react";
import { useLocation, useParams } from "react-router-dom";
import { addonIframeManager } from "./addon-iframe-manager";

interface AddonIframeRouteProps {
  addonId: string;
  routeId: string;
}

export function AddonIframeRoute({ addonId, routeId }: AddonIframeRouteProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const location = useLocation();
  const params = useParams();

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return undefined;
    }

    addonIframeManager.attachRoute(addonId, container);

    return () => {
      addonIframeManager.detachRoute(addonId, container);
    };
  }, [addonId]);

  useEffect(() => {
    addonIframeManager.updateRoute(addonId, routeId, {
      hash: location.hash,
      params,
      pathname: location.pathname,
      search: location.search,
    });
  }, [addonId, routeId, location.hash, location.pathname, location.search, params]);

  return (
    <div
      ref={containerRef}
      className="min-h-[calc(100vh-96px)] w-full overflow-hidden"
      data-addon-id={addonId}
      data-addon-route-id={routeId}
    />
  );
}
