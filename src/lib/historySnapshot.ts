/** Compare ordered IPC row snapshots without serializing them or assuming a field list. */
export function equalOrderedSnapshots<T extends object>(left: readonly T[], right: readonly T[]): boolean {
  if (left === right) return true;
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index++) {
    const a = left[index];
    const b = right[index];
    if (a === b) continue;
    const aKeys = Reflect.ownKeys(a);
    const bKeys = Reflect.ownKeys(b);
    if (aKeys.length !== bKeys.length) return false;
    for (const key of aKeys) {
      if (!Object.prototype.hasOwnProperty.call(b, key) || !Object.is(a[key as keyof T], b[key as keyof T])) return false;
    }
  }
  return true;
}
