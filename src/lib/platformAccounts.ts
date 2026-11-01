import type { PlatformAccount, UploadTarget } from "../types";
export function platformTargets(targets: UploadTarget[], accounts: PlatformAccount[] = []) {
  const seen = new Set<string>();
  return targets.filter(target => {
    const primary = accounts.find(account => account.driverId === target.driverId);
    if (primary && primary.targetId !== target.id) return false;
    if (seen.has(target.driverId)) return false;
    seen.add(target.driverId); return true;
  });
}
export function normalizePlatforms(ids: string[], targets: UploadTarget[], accounts: PlatformAccount[] = []) {
  const visible = platformTargets(targets, accounts);
  return [...new Set(ids.flatMap(id => {
    const target = targets.find(t => t.id === id);
    const primary = visible.find(t => t.driverId === target?.driverId);
    return primary ? [primary.id] : [];
  }))];
}
