export class Point {
  constructor(public x: number, public y: number) {}

  shift(dx: number): Point {
    return new Point(this.x + dx, this.y);
  }
}

export function origin(): Point {
  return new Point(0, 0);
}
