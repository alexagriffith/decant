export function compareCodePoints(left: string, right: string): number {
  const leftIterator = left[Symbol.iterator]();
  const rightIterator = right[Symbol.iterator]();
  for (;;) {
    const leftNext = leftIterator.next();
    const rightNext = rightIterator.next();
    if (leftNext.done && rightNext.done) {
      return 0;
    }
    if (leftNext.done) {
      return -1;
    }
    if (rightNext.done) {
      return 1;
    }
    const leftCodePoint = leftNext.value.codePointAt(0) as number;
    const rightCodePoint = rightNext.value.codePointAt(0) as number;
    if (leftCodePoint !== rightCodePoint) {
      return leftCodePoint - rightCodePoint;
    }
  }
}
