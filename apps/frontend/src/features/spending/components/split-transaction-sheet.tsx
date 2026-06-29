import { useEffect, useMemo, useState } from "react";

import {
  Button,
  Icons,
  Input,
  MoneyInput,
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@wealthfolio/ui";
import { useIsMobileViewport } from "@/hooks/use-platform";
import type { TaxonomyCategory } from "@/lib/types";
import { cn } from "@/lib/utils";

import { QuickCategorizePopover, type QuickCategorizeScope } from "./quick-categorize-popover";
import {
  canDistributeSplitCents,
  centsToAmount,
  distributeEvenlyCents,
  distributeRemainingCents,
  toCents,
} from "../lib/split-utils";
import type { TransactionRowVM } from "../lib/transactions-helpers";
import type { NewActivitySplit } from "../types/cash-activity";

const SPENDING_TAXONOMY = "spending_categories";
const INCOME_TAXONOMY = "income_sources";
const SAVINGS_TAXONOMY = "savings_categories";

interface SplitLineState {
  id: string;
  taxonomyId: string;
  categoryId: string;
  amount: number | undefined;
  note: string;
}

interface SplitTransactionSheetProps {
  open: boolean;
  row: TransactionRowVM | null;
  categories: Map<string, TaxonomyCategory>;
  isSaving: boolean;
  onOpenChange: (open: boolean) => void;
  onSave: (activityId: string, splits: NewActivitySplit[]) => Promise<void>;
  onClear: (activityId: string) => Promise<void>;
}

function taxonomyForBucket(bucket: string | undefined): string | null {
  if (bucket === "spending") return SPENDING_TAXONOMY;
  if (bucket === "income") return INCOME_TAXONOMY;
  if (bucket === "saving") return SAVINGS_TAXONOMY;
  return null;
}

function scopeForTaxonomy(taxonomyId: string | null): QuickCategorizeScope {
  if (taxonomyId === INCOME_TAXONOMY) return "income";
  if (taxonomyId === SAVINGS_TAXONOMY) return "saving";
  return "expense";
}

function makeLine(taxonomyId: string, categoryId = "", amount?: number, note = ""): SplitLineState {
  return {
    id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    taxonomyId,
    categoryId,
    amount,
    note,
  };
}

function categoryLabel(
  category: TaxonomyCategory | undefined,
  categories: Map<string, TaxonomyCategory>,
) {
  if (!category) return null;
  const parent = category.parentId ? categories.get(category.parentId) : null;
  return parent ? `${parent.name} / ${category.name}` : category.name;
}

export function SplitTransactionSheet({
  open,
  row,
  categories,
  isSaving,
  onOpenChange,
  onSave,
  onClear,
}: SplitTransactionSheetProps) {
  const isMobile = useIsMobileViewport();
  const activity = row?.activity ?? null;
  const taxonomyId = taxonomyForBucket(activity?.cashFlowBucket);
  const totalCents = toCents(activity?.amount);
  const totalAmount = Math.abs(totalCents) / 100;

  const [lines, setLines] = useState<SplitLineState[]>([]);

  useEffect(() => {
    if (!open || !row || !taxonomyId) return;
    const existing = (row.activity.splits ?? [])
      .filter((split) => split.taxonomyId === taxonomyId)
      .sort((a, b) => a.sortOrder - b.sortOrder);
    if (existing.length > 0) {
      setLines(
        existing.map((split) =>
          makeLine(
            split.taxonomyId,
            split.categoryId,
            Math.abs(Number(split.amount)),
            split.note ?? "",
          ),
        ),
      );
      return;
    }
    if (row.category) {
      setLines([makeLine(row.category.taxonomyId, row.category.id, totalAmount)]);
      return;
    }
    setLines([makeLine(taxonomyId, "", totalAmount)]);
  }, [open, row, taxonomyId, totalAmount]);

  const assignedCents = useMemo(
    () => lines.reduce((sum, line) => sum + toCents(line.amount), 0),
    [lines],
  );
  const totalAbsCents = Math.abs(totalCents);
  const remainingCents = totalAbsCents - assignedCents;
  const emptyAmountIndexes = lines
    .map((line, index) => ({ line, index }))
    .filter(({ line }) => toCents(line.amount) <= 0)
    .map(({ index }) => index);
  const hasInvalidLine = lines.some((line) => !line.categoryId || toCents(line.amount) <= 0);
  const canSave =
    !!activity && !!taxonomyId && lines.length > 0 && !hasInvalidLine && remainingCents === 0;
  const canDistribute = canDistributeSplitCents(
    totalAbsCents,
    assignedCents,
    emptyAmountIndexes.length,
    lines.length,
  );
  const hasExistingSplits = (row?.splitCount ?? 0) > 0;

  const updateLine = (id: string, patch: Partial<SplitLineState>) => {
    setLines((current) => current.map((line) => (line.id === id ? { ...line, ...patch } : line)));
  };

  const handleAddLine = () => {
    if (!taxonomyId) return;
    setLines((current) => [...current, makeLine(taxonomyId)]);
  };

  const handleRemoveLine = (id: string) => {
    setLines((current) =>
      current.length > 1 ? current.filter((line) => line.id !== id) : current,
    );
  };

  const handleDistribute = () => {
    if (!canDistribute) return;
    if (remainingCents > 0) {
      const amounts = distributeRemainingCents(
        totalAbsCents,
        assignedCents,
        emptyAmountIndexes.length,
      );
      setLines((current) =>
        current.map((line, index) => {
          const emptyIndex = emptyAmountIndexes.indexOf(index);
          return emptyIndex >= 0 ? { ...line, amount: centsToAmount(amounts[emptyIndex]) } : line;
        }),
      );
      return;
    }

    const amounts = distributeEvenlyCents(totalAbsCents, lines.length);
    setLines((current) =>
      current.map((line, index) => ({ ...line, amount: centsToAmount(amounts[index]) })),
    );
  };

  const handleSave = async () => {
    if (!canSave || !activity || !taxonomyId) return;
    await onSave(
      activity.id,
      lines.map((line, index) => ({
        taxonomyId,
        categoryId: line.categoryId,
        amount: (line.amount ?? 0).toFixed(2),
        note: line.note.trim() || null,
        sortOrder: index,
      })),
    );
    onOpenChange(false);
  };

  const handleClear = async () => {
    if (!activity) return;
    await onClear(activity.id);
    onOpenChange(false);
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent
        side={isMobile ? "bottom" : "right"}
        className={cn(
          "flex w-full flex-col overflow-hidden",
          isMobile ? "rounded-t-4xl mx-1 h-[90vh] gap-0 p-0" : "sm:max-w-xl",
        )}
      >
        <SheetHeader className={cn(isMobile && "border-border border-b px-6 py-4 text-left")}>
          <SheetTitle>Split Transaction</SheetTitle>
          <SheetDescription>
            {activity?.notes ?? "Transaction"} · {activity?.currency ?? ""}
          </SheetDescription>
        </SheetHeader>

        <div
          className={cn(
            "min-h-0 flex-1 space-y-4 overflow-y-auto py-4",
            isMobile ? "px-4" : "px-1",
          )}
        >
          <div className="border-border/70 bg-muted/30 flex items-center justify-between rounded-md border px-3 py-2 text-sm">
            <span className="text-muted-foreground">Remaining</span>
            <span
              className={cn(
                "font-medium tabular-nums",
                remainingCents === 0
                  ? "text-success"
                  : remainingCents < 0
                    ? "text-destructive"
                    : "text-foreground",
              )}
            >
              {centsToAmount(Math.abs(remainingCents)).toFixed(2)} {activity?.currency}
            </span>
          </div>

          <div className="space-y-2">
            {lines.map((line, index) => {
              const category = categories.get(line.categoryId);
              const label = categoryLabel(category, categories);
              return (
                <div key={line.id} className="border-border/70 rounded-md border p-3">
                  <div className="grid gap-2 sm:grid-cols-[1fr_128px_32px]">
                    <QuickCategorizePopover
                      scope={scopeForTaxonomy(taxonomyId)}
                      selectedCategoryId={line.categoryId || null}
                      onSelect={(nextTaxonomyId, categoryId) =>
                        updateLine(line.id, { taxonomyId: nextTaxonomyId, categoryId })
                      }
                      trigger={
                        <button
                          type="button"
                          className="border-input bg-input-bg dark:bg-input/30 hover:bg-accent/30 h-input-height flex min-w-0 items-center justify-between rounded-md border px-3 py-2 text-sm"
                        >
                          {label ? (
                            <span className="flex min-w-0 items-center gap-2">
                              {category?.color && (
                                <span
                                  className="h-2.5 w-2.5 shrink-0 rounded-full"
                                  style={{ backgroundColor: category.color }}
                                  aria-hidden="true"
                                />
                              )}
                              <span className="truncate">{label}</span>
                            </span>
                          ) : (
                            <span className="text-muted-foreground">Category</span>
                          )}
                          <Icons.ChevronDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
                        </button>
                      }
                    />
                    <MoneyInput
                      value={line.amount ?? 0}
                      onValueChange={(value: number | undefined) =>
                        updateLine(line.id, { amount: value ?? undefined })
                      }
                      placeholder="0.00"
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-input-height w-8"
                      onClick={() => handleRemoveLine(line.id)}
                      disabled={lines.length <= 1}
                      aria-label={`Remove split line ${index + 1}`}
                    >
                      <Icons.Trash className="h-4 w-4" />
                    </Button>
                  </div>
                  <Input
                    value={line.note}
                    onChange={(event) => updateLine(line.id, { note: event.target.value })}
                    placeholder="Note"
                    className="mt-2"
                  />
                </div>
              );
            })}
          </div>

          <div className="flex flex-wrap gap-2">
            <Button type="button" variant="outline" size="sm" onClick={handleAddLine}>
              <Icons.Plus className="mr-2 h-4 w-4" />
              Add line
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleDistribute}
              disabled={!canDistribute}
            >
              <Icons.SplitHorizontal className="mr-2 h-4 w-4" />
              Distribute evenly
            </Button>
          </div>
        </div>

        <SheetFooter
          className={cn(
            "border-t",
            isMobile
              ? "border-border px-4 py-4 pb-[calc(env(safe-area-inset-bottom,0px)+1rem)]"
              : "gap-2 pt-4",
          )}
        >
          {isMobile ? (
            <div className="flex w-full flex-col gap-2">
              <Button type="button" onClick={handleSave} disabled={!canSave || isSaving}>
                {isSaving && <Icons.Spinner className="mr-2 h-4 w-4 animate-spin" />}
                Save
              </Button>
              <div className="flex gap-2">
                {hasExistingSplits && (
                  <Button
                    type="button"
                    variant="outline"
                    onClick={handleClear}
                    disabled={isSaving}
                    className="flex-1"
                  >
                    Clear split
                  </Button>
                )}
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => onOpenChange(false)}
                  disabled={isSaving}
                  className="flex-1"
                >
                  Cancel
                </Button>
              </div>
            </div>
          ) : (
            <>
              {hasExistingSplits && (
                <Button
                  type="button"
                  variant="outline"
                  onClick={handleClear}
                  disabled={isSaving}
                  className="mr-auto"
                >
                  Clear split
                </Button>
              )}
              <Button
                type="button"
                variant="outline"
                onClick={() => onOpenChange(false)}
                disabled={isSaving}
              >
                Cancel
              </Button>
              <Button type="button" onClick={handleSave} disabled={!canSave || isSaving}>
                {isSaving && <Icons.Spinner className="mr-2 h-4 w-4 animate-spin" />}
                Save
              </Button>
            </>
          )}
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
