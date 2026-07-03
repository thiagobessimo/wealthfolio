import { usePersistentState } from "@/hooks/use-persistent-state";
import { usePlatform } from "@/hooks/use-platform";
import { useSettingsContext } from "@/lib/settings-provider";
import { cn } from "@/lib/utils";
import {
  NAVIGATION_MODE_STORAGE_KEY,
  type NavigationMode,
} from "@/pages/layouts/navigation/navigation-mode-context";
import { Icons } from "@wealthfolio/ui";
import { Card, CardContent } from "@wealthfolio/ui/components/ui/card";
import { forwardRef, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

export interface OnboardingAppearanceHandle {
  submitForm: () => void;
}

interface OnboardingAppearanceProps {
  onNext: () => void;
  onValidityChange: (isValid: boolean) => void;
}

export const OnboardingAppearance = forwardRef<
  OnboardingAppearanceHandle,
  OnboardingAppearanceProps
>(({ onNext, onValidityChange }, ref) => {
  const { t } = useTranslation();
  const { settings, updateSettings } = useSettingsContext();
  const fonts = useMemo(
    () => [
      {
        value: "font-mono",
        label: t("onboarding:appearance.fonts.monoLabel"),
        description: t("onboarding:appearance.fonts.monoDescription"),
      },
      {
        value: "font-sans",
        label: t("onboarding:appearance.fonts.sansLabel"),
        description: t("onboarding:appearance.fonts.sansDescription"),
      },
      {
        value: "font-serif",
        label: t("onboarding:appearance.fonts.serifLabel"),
        description: t("onboarding:appearance.fonts.serifDescription"),
      },
    ],
    [t],
  );
  const [theme, setTheme] = useState<string>(settings?.theme ?? "system");
  const [font, setFont] = useState<string>(settings?.font ?? "font-mono");
  const { isMobile } = usePlatform();
  // Navigation style lives in localStorage (read by NavigationModeProvider on app
  // load); it only applies on large screens, so the picker is desktop-only.
  const [navMode, setNavMode] = usePersistentState<NavigationMode>(
    NAVIGATION_MODE_STORAGE_KEY,
    "sidebar",
  );

  useEffect(() => {
    // Always valid since we have defaults
    onValidityChange(true);
  }, [onValidityChange]);

  useImperativeHandle(ref, () => ({
    submitForm() {
      updateSettings({ theme, font })
        .then(() => onNext())
        .catch((error) => console.error("Failed to save appearance settings:", error));
    },
  }));

  // Apply theme/font preview when user selects them
  const handleThemeChange = (newTheme: string) => {
    setTheme(newTheme);
    updateSettings({ theme: newTheme }).catch(console.error);
  };

  const handleFontChange = (newFont: string) => {
    setFont(newFont);
    updateSettings({ font: newFont }).catch(console.error);
  };

  return (
    <div className="w-full max-w-2xl space-y-8">
      <div className="text-center">
        <p className="text-muted-foreground">{t("onboarding:appearance.subtitle")}</p>
      </div>

      <Card className="border-none bg-transparent">
        <CardContent className="space-y-10 p-0 sm:p-6">
          {/* Theme Selection */}
          <div>
            <div className="mb-5 flex items-center gap-3">
              <div className="bg-muted rounded-lg p-2">
                <Icons.Palette className="text-muted-foreground h-5 w-5" />
              </div>
              <span className="text-xl font-semibold">{t("onboarding:appearance.themeLabel")}</span>
            </div>

            <div className="grid grid-cols-3 gap-3 sm:gap-4">
              {/* Light Theme */}
              <button
                type="button"
                data-testid="theme-light-button"
                onClick={() => handleThemeChange("light")}
                className={cn(
                  "group relative overflow-hidden rounded-xl border-2 transition-all duration-200",
                  theme === "light"
                    ? "border-primary ring-primary/20 ring-2"
                    : "border-border hover:border-primary/50",
                )}
              >
                <div className="overflow-hidden rounded-t-lg">
                  <img
                    src="/themes/theme-light.webp"
                    srcSet="/themes/theme-light.webp 1x, /themes/theme-light@2x.webp 2x"
                    alt={t("onboarding:appearance.themeLightPreviewAlt")}
                    className="h-auto w-full object-cover"
                  />
                </div>
                <div
                  className={cn(
                    "flex items-center justify-center gap-2 py-2.5 sm:py-3",
                    theme === "light" ? "bg-primary/10" : "bg-muted/50",
                  )}
                >
                  <Icons.Sun
                    className={cn(
                      "h-4 w-4",
                      theme === "light" ? "text-primary" : "text-muted-foreground",
                    )}
                  />
                  <span className="text-sm font-medium">
                    {t("onboarding:appearance.themeLight")}
                  </span>
                </div>
                {theme === "light" && (
                  <div className="bg-primary absolute right-2 top-2 rounded-full p-0.5">
                    <Icons.Check className="h-3 w-3 text-white" />
                  </div>
                )}
              </button>

              {/* Dark Theme */}
              <button
                type="button"
                onClick={() => handleThemeChange("dark")}
                className={cn(
                  "group relative overflow-hidden rounded-xl border-2 transition-all duration-200",
                  theme === "dark"
                    ? "border-primary ring-primary/20 ring-2"
                    : "border-border hover:border-primary/50",
                )}
              >
                <div className="overflow-hidden rounded-t-lg">
                  <img
                    src="/themes/theme-dark.webp"
                    srcSet="/themes/theme-dark.webp 1x, /themes/theme-dark@2x.webp 2x"
                    alt={t("onboarding:appearance.themeDarkPreviewAlt")}
                    className="h-auto w-full object-cover"
                  />
                </div>
                <div
                  className={cn(
                    "flex items-center justify-center gap-2 py-2.5 sm:py-3",
                    theme === "dark" ? "bg-primary/10" : "bg-muted/50",
                  )}
                >
                  <Icons.Moon
                    className={cn(
                      "h-4 w-4",
                      theme === "dark" ? "text-primary" : "text-muted-foreground",
                    )}
                  />
                  <span className="text-sm font-medium">
                    {t("onboarding:appearance.themeDark")}
                  </span>
                </div>
                {theme === "dark" && (
                  <div className="bg-primary absolute right-2 top-2 rounded-full p-0.5">
                    <Icons.Check className="h-3 w-3 text-white" />
                  </div>
                )}
              </button>

              {/* System Theme */}
              <button
                type="button"
                onClick={() => handleThemeChange("system")}
                className={cn(
                  "group relative overflow-hidden rounded-xl border-2 transition-all duration-200",
                  theme === "system"
                    ? "border-primary ring-primary/20 ring-2"
                    : "border-border hover:border-primary/50",
                )}
              >
                <div className="overflow-hidden rounded-t-lg">
                  <img
                    src="/themes/theme-system.webp"
                    srcSet="/themes/theme-system.webp 1x, /themes/theme-system@2x.webp 2x"
                    alt={t("onboarding:appearance.themeSystemPreviewAlt")}
                    className="h-auto w-full object-cover"
                  />
                </div>
                <div
                  className={cn(
                    "flex items-center justify-center gap-2 py-2.5 sm:py-3",
                    theme === "system" ? "bg-primary/10" : "bg-muted/50",
                  )}
                >
                  <Icons.Monitor
                    className={cn(
                      "h-4 w-4",
                      theme === "system" ? "text-primary" : "text-muted-foreground",
                    )}
                  />
                  <span className="text-sm font-medium">
                    {t("onboarding:appearance.themeSystem")}
                  </span>
                </div>
                {theme === "system" && (
                  <div className="bg-primary absolute right-2 top-2 rounded-full p-0.5">
                    <Icons.Check className="h-3 w-3 text-white" />
                  </div>
                )}
              </button>
            </div>
          </div>

          {/* Navigation Style — desktop only (mobile always uses the mobile nav) */}
          {!isMobile && (
            <div>
              <div className="mb-5 flex items-center gap-3">
                <div className="bg-muted rounded-lg p-2">
                  <Icons.PanelLeft className="text-muted-foreground h-5 w-5" />
                </div>
                <span className="text-xl font-semibold">
                  {t("onboarding:appearance.navigationLabel")}
                </span>
              </div>

              <div className="grid grid-cols-2 gap-3 sm:gap-4">
                {/* Sidebar */}
                <button
                  type="button"
                  data-testid="nav-sidebar-button"
                  onClick={() => setNavMode("sidebar")}
                  className={cn(
                    "group relative overflow-hidden rounded-xl border-2 transition-all duration-200",
                    navMode === "sidebar"
                      ? "border-primary ring-primary/20 ring-2"
                      : "border-border hover:border-primary/50",
                  )}
                >
                  <div className="bg-muted/40 aspect-video w-full overflow-hidden p-2.5">
                    <div className="flex h-full gap-1.5">
                      <div className="bg-foreground/10 flex w-[18%] flex-col items-center gap-1.5 rounded-md py-2">
                        <div className="bg-foreground/40 h-2.5 w-2.5 rounded-[4px]" />
                        <div className="bg-foreground/10 my-0.5 h-px w-1/2 rounded-full" />
                        <div className="bg-foreground/45 h-2.5 w-2.5 rounded-full" />
                        <div className="bg-foreground/20 h-2.5 w-2.5 rounded-full" />
                        <div className="bg-foreground/20 h-2.5 w-2.5 rounded-full" />
                        <div className="bg-foreground/20 h-2.5 w-2.5 rounded-full" />
                      </div>
                      <div className="bg-foreground/5 flex flex-1 flex-col gap-1.5 rounded-md p-2">
                        <div className="bg-foreground/20 h-2 w-1/2 rounded-full" />
                        <div className="grid flex-1 grid-cols-3 gap-1.5">
                          <div className="bg-foreground/10 rounded" />
                          <div className="bg-foreground/10 rounded" />
                          <div className="bg-foreground/10 rounded" />
                        </div>
                      </div>
                    </div>
                  </div>
                  <div
                    className={cn(
                      "flex items-center justify-center gap-2 py-2.5 sm:py-3",
                      navMode === "sidebar" ? "bg-primary/10" : "bg-muted/50",
                    )}
                  >
                    <Icons.PanelLeft
                      className={cn(
                        "h-4 w-4",
                        navMode === "sidebar" ? "text-primary" : "text-muted-foreground",
                      )}
                    />
                    <span className="text-sm font-medium">
                      {t("onboarding:appearance.navSidebar")}
                    </span>
                  </div>
                  {navMode === "sidebar" && (
                    <div className="bg-primary absolute right-2 top-2 rounded-full p-0.5">
                      <Icons.Check className="h-3 w-3 text-white" />
                    </div>
                  )}
                </button>

                {/* Floating Bar */}
                <button
                  type="button"
                  data-testid="nav-launchbar-button"
                  onClick={() => setNavMode("launchbar")}
                  className={cn(
                    "group relative overflow-hidden rounded-xl border-2 transition-all duration-200",
                    navMode === "launchbar"
                      ? "border-primary ring-primary/20 ring-2"
                      : "border-border hover:border-primary/50",
                  )}
                >
                  <div className="bg-muted/40 aspect-video w-full overflow-hidden p-2.5">
                    <div className="bg-foreground/5 relative h-full rounded-md p-2">
                      <div className="bg-foreground/20 h-2 w-2/5 rounded-full" />
                      <div className="mt-1.5 grid grid-cols-3 gap-1.5">
                        <div className="bg-foreground/10 h-7 rounded" />
                        <div className="bg-foreground/10 h-7 rounded" />
                        <div className="bg-foreground/10 h-7 rounded" />
                      </div>
                      <div className="bg-foreground/80 absolute bottom-2 left-1/2 flex -translate-x-1/2 items-center gap-1.5 rounded-full px-2.5 py-1.5 shadow-sm">
                        <div className="bg-background h-1.5 w-1.5 rounded-full" />
                        <div className="bg-background/45 h-1.5 w-1.5 rounded-full" />
                        <div className="bg-background/45 h-1.5 w-1.5 rounded-full" />
                        <div className="bg-background/45 h-1.5 w-1.5 rounded-full" />
                        <div className="bg-background/45 h-1.5 w-1.5 rounded-full" />
                      </div>
                    </div>
                  </div>
                  <div
                    className={cn(
                      "flex items-center justify-center gap-2 py-2.5 sm:py-3",
                      navMode === "launchbar" ? "bg-primary/10" : "bg-muted/50",
                    )}
                  >
                    <Icons.RectangleEllipsis
                      className={cn(
                        "h-4 w-4",
                        navMode === "launchbar" ? "text-primary" : "text-muted-foreground",
                      )}
                    />
                    <span className="text-sm font-medium">
                      {t("onboarding:appearance.navFloating")}
                    </span>
                  </div>
                  {navMode === "launchbar" && (
                    <div className="bg-primary absolute right-2 top-2 rounded-full p-0.5">
                      <Icons.Check className="h-3 w-3 text-white" />
                    </div>
                  )}
                </button>
              </div>
            </div>
          )}

          {/* Font Selection */}
          <div>
            <div className="mb-5 flex items-center gap-3">
              <div className="bg-muted rounded-lg p-2">
                <Icons.Type className="text-muted-foreground h-5 w-5" />
              </div>
              <span className="text-xl font-semibold">{t("onboarding:appearance.fontLabel")}</span>
            </div>

            <div className="grid grid-cols-3 gap-3 sm:gap-4">
              {fonts.map((f) => (
                <button
                  key={f.value}
                  type="button"
                  onClick={() => handleFontChange(f.value)}
                  className={cn(
                    "group relative flex flex-col overflow-hidden rounded-xl border-2 transition-all duration-200",
                    font === f.value
                      ? "border-primary ring-primary/20 ring-2"
                      : "border-border hover:border-primary/50",
                    f.value,
                  )}
                >
                  {/* Font preview area */}
                  <div className="bg-muted/30 flex flex-1 flex-col items-center justify-center px-3 py-3 text-center sm:px-4 sm:py-4">
                    <div className="w-full space-y-2">
                      {/* Font name as hero */}
                      <div className="text-xl font-medium tracking-tight sm:text-2xl">
                        {f.label}
                      </div>
                      {/* Sample text paragraph */}
                      <div className="text-muted-foreground text-[11px] leading-relaxed sm:text-xs">
                        {t("onboarding:appearance.fontSample")}
                      </div>
                      {/* Secondary: numbers sample */}
                      <div className="text-muted-foreground/60 whitespace-nowrap text-[10px] sm:text-xs">
                        12345 · $1,234
                      </div>
                    </div>
                  </div>
                  {/* Label area */}
                  <div
                    className={cn(
                      "w-full px-4 py-2.5 text-center sm:py-3",
                      font === f.value ? "bg-primary/10" : "bg-muted/50",
                    )}
                  >
                    <div className="text-muted-foreground text-xs">{f.description}</div>
                  </div>
                  {font === f.value && (
                    <div className="bg-primary absolute right-2 top-2 rounded-full p-0.5">
                      <Icons.Check className="h-3 w-3 text-white" />
                    </div>
                  )}
                </button>
              ))}
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
});

OnboardingAppearance.displayName = "OnboardingAppearance";
