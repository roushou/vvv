import { Point, area } from './index';
import { shapeArea } from './geometry/shape';

const p = new Point(1, 2);
console.log(shapeArea({ corner: p, side: 3 }), area(2), p.shift(1));
