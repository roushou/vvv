import { Point } from './point';
import { area } from '../util';

export interface Shape {
  corner: Point;
  side: number;
}

export function shapeArea(shape: Shape): number {
  return area(shape.side);
}
