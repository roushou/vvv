import { seed, later, first, run } from './origin';
export function simple(input) {
  const local = input;
  let next;
  next = local;
  return [input, local, next];
}
export function before(input) {
  later;
  const later = seed;
  return later;
}
export function self(input) {
  const first = first;
  return first;
}
export function nested(input) {
  const local = input;
  {
    const local = seed;
    local;
  }
  return local;
}
export function innerWins(input) {
  {
    const later = seed;
    later;
  }
  const later = input;
  return later;
}
export function patterns(input) {
  const { label: renamed, nested: { value }, short = seed, ...rest } = input;
  let [head = renamed, , ...tail] = input;
  return [renamed, value, short, rest, head, tail];
}
export function order(input) {
  const { first = seed, second = first, [second]: third } = input;
  return [first, second, third];
}
export function forward(input) {
  const { first = later, later = seed } = input;
  return [first, later];
}
export function whole(input) {
  const { first } = first;
  return first;
}
export function multiple(input) {
  let first = input, second = first;
  return [first, second];
}
export function peers(input) {
  let first = later, later = input;
  return [first, later];
}
export function callable(input) {
  const { run } = input;
  run();
}
export function duplicates(input) {
  let [same, same] = input;
  return same;
}
export function invalid(input) {
  const good = input;
  const [first, ...rest, after] = input;
  return good;
}
export function hoisted(input) {
  var local = input;
  return input;
}
export function looping(input) {
  for (let item of input) { item; }
  return input;
}
export function signature(value) {
  const local = value;
  return local;
}
