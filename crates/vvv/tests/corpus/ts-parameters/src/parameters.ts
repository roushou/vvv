import { seed, later, first, original, run } from './origin';
export function object({ original: renamed, nested: { value }, short, count = seed, ...rest }, [head = renamed, , ...tail]) {
  return [renamed, value, short, count, rest, head, tail];
}
export function ordered({ first = seed, second = first }, third = second) {
  return [first, second, third];
}
export function forward({ first = later }, later = seed) {
  return [first, later];
}
export function self({ first = first }) {
  return first;
}
export function computed({ [seed]: selected, [selected]: last }) {
  return [selected, last];
}
export function whole({ first = seed } = first) {
  return first;
}
export function callable({ run }) {
  run();
}
export function duplicates([same, same]) {
  return same;
}
export function invalid([supported, ...tail, after]) {
  return supported;
}
export function locals({ first }) {
  const local = first;
  return first;
}
export function anonymous({ first }) {
  return () => first;
}
