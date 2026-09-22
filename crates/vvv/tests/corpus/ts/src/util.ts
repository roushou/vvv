export function area(side: number): number {
  return side * side;
}

function unusedPrivate(): number {
  return area(1);
}

export const VERSION = 1;
