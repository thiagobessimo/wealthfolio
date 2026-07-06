# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [3.6.0] - 2026-07-04

Addons now run in an isolated sandbox iframe. This is a **breaking** release for
addons that register routes. See the
[v3.5 → v3.6 migration guide](../../docs/addons/addon-migration-guide-v3.5-to-v3.6.md).

### Breaking

- **Route registration**: `RouteConfig.component` (a lazy React component) is
  replaced by `RouteConfig.render(ctx)`, which mounts into a host-provided
  `ctx.root: HTMLElement`. New types: `AddonRouteRenderer`,
  `AddonRouteRenderContext`, `AddonRouteLocation`.
- **React exports removed**: the SDK no longer re-exports `React` / `ReactDOM`
  from host globals. Import from `react` / `react-dom/client`; mark them
  `external` in your build.
- **`SidebarItemConfig`**: `icon` is now a typed `AddonIconName` (one of a
  curated set of duotone Phosphor icons the host bundles) — `React.ReactNode`
  icons and the `onClick` handler were removed. Use `route`. Unknown/omitted
  names render a neutral `caret-right` fallback.
- **React** guaranteed version bumped 19.1.1 → 19.2.4.

### Added

- `HOST_DEPENDENCIES` export — the versioned packages the sandbox provides
  (react, react-dom, @tanstack/react-query, @wealthfolio/ui, date-fns,
  lucide-react, recharts).
- `AddonIconName` type and `ADDON_ICON_NAMES` list — the 80 supported sidebar
  icon names.
- Brokered networking: `ctx.api.network.request()` with manifest-declared
  `network.allowedHosts` and bearer auth resolved from scoped secret storage
  (`NetworkAPI`, `NetworkRequest`, `NetworkResponse`, `NetworkAuth`).
- Manifest fields: `minWealthfolioVersion`, `hostDependencies`, `network`,
  `sha256`. `AddonHostDependencies` type.
- Addon SDK tax fields on activity/data types (#1188).

## [1.0.0] - 2024-12-19

### Added

- **Initial Release** - Complete TypeScript SDK for building Wealthfolio addons
- **Core Types**: AddonContext, SidebarManager, RouterManager, and event
  handling
- **Data Types**: Comprehensive financial data models (Account, Activity, Asset,
  Holding, etc.)
- **Permission System**: Risk-based permission categorization with validation
  and security controls
- **Manifest Management**: Validation, compatibility checks, and metadata
  handling
- **Host API Interface**: Secure communication layer between addons and
  Wealthfolio
- **Utility Functions**: Addon validation, version compatibility, ID generation,
  and size formatting
- **Development Tools**: Complete TypeScript definitions and development
  utilities

### Features

- Enhanced type safety with complete data type definitions
- Permission system with risk-based categorization (low, medium, high)
- Addon lifecycle management (install, validate, update, enable/disable)
- Development and runtime manifest types
- Comprehensive export definitions for all addon functionality
- Support for addon store listings and metadata

### Technical

- ESM module format with TypeScript declarations
- React peer dependency support (^18.0.0)
- Node.js 20+ compatibility
- MIT licensed for maximum compatibility
- Complete build pipeline with tsup
- Comprehensive type exports and module structure
