import { outer } from "./helpers";
export function basic(seed = outer) {
  outer;
  if (seed) { var outer = seed; }
  const capture = () => outer;
  { let outer = 1; outer; }
  return outer;
}
export function patterns(input) {
  first; tail;
  var { value: first = first, [first]: second = tail, ...rest } = input;
  var [head, , ...tail] = input;
  return second;
}
export function loops(entries) {
  index; item;
  for (var index = 0; index < 2; index++) { index; }
  for (var { name: item = index } of entries) { item; }
  for (var key in entries) { key; }
  return item;
}
export function duplicates() {
  value;
  var value = 1;
  var value = 2;
}
export function isolated(input) {
  const inner = () => { local; var local = input; return local; };
  return input;
}
export function conflict(value) {
  var value;
  return value;
}
export function functionConflict() {
  var helper;
  function helper() {}
  return helper;
}
export function malformed(input) {
  var good = input;
  var [bad, ...rest, last] = input;
  return good;
}
export function legacy(entries) {
  for (var item = 1 in entries) { item; }
  return entries;
}
export async function awaited(entries) {
  for await (var item of entries) { item; }
  return item;
}
outer;
