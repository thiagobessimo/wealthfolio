import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { listSellConstraints, saveSellConstraints } from "@/adapters";
import type { RebalanceSellConstraint, SellConstraintEntityType } from "@/lib/types";

export function useSellConstraints(targetId: string | undefined) {
  const queryClient = useQueryClient();
  const queryKey = ["sell-constraints", targetId];

  const query = useQuery({
    queryKey,
    queryFn: () => listSellConstraints(targetId!),
    enabled: !!targetId,
    staleTime: Infinity,
  });

  const mutation = useMutation({
    mutationFn: (constraints: RebalanceSellConstraint[]) =>
      saveSellConstraints(targetId!, constraints),
    onSuccess: (data) => {
      queryClient.setQueryData(queryKey, data);
    },
  });

  const constraints = query.data ?? [];

  const doNotSellAssetIds = constraints
    .filter((c) => c.entityType === "asset")
    .map((c) => c.entityId);

  const avoidSellingAccountIds = constraints
    .filter((c) => c.entityType === "account")
    .map((c) => c.entityId);

  function toggleConstraint(entityType: SellConstraintEntityType, entityId: string) {
    const existing = constraints.find(
      (c) => c.entityType === entityType && c.entityId === entityId,
    );
    const next = existing
      ? constraints.filter((c) => c.id !== existing.id)
      : [
          ...constraints,
          {
            id: crypto.randomUUID(),
            targetId: targetId!,
            entityType,
            entityId,
            createdAt: new Date().toISOString(),
          } satisfies RebalanceSellConstraint,
        ];
    mutation.mutate(next);
  }

  function hasConstraint(entityType: SellConstraintEntityType, entityId: string): boolean {
    return constraints.some((c) => c.entityType === entityType && c.entityId === entityId);
  }

  return {
    constraints,
    doNotSellAssetIds,
    avoidSellingAccountIds,
    toggleConstraint,
    hasConstraint,
    isLoading: query.isLoading,
  };
}
