export function classic(input) {
  for (let index = input, next = index; index < next; index++) {
    index;
    { const index = input; index; }
  }
  index;
  return input;
}
export function self(input) {
  for (let input = input; input; input++) { input; }
  return input;
}
export function forward(input) {
  for (let first = later, later = input; first; later++) { first; }
  return input;
}
export function each(input) {
  for (const { label: renamed, first = input, second = first, ...rest } of input) {
    renamed; second; rest;
  }
  renamed;
  return input;
}
export function iterable(input) {
  for (let input of input) { input; }
  return input;
}
export function keys(input) {
  for (const key in input) key;
  return input;
}
export async function awaited(input) {
  for await (const [head, ...tail] of input) { head; tail; }
  return input;
}
export function duplicate(input) {
  for (let [same, same] of input) { same; }
}
export function invalid(input) {
  for (const [good, ...rest, last] of input) { good; }
  return input;
}
export function hoisted(input) {
  for (var item of input) { item; }
  return input;
}
export function assignment(input, existing) {
  for (existing of input) { existing; }
  return existing;
}
export function empty(input) {
  for (;;) { input; break; }
  return input;
}
export function malformed(input) {
  for (let good = input; input + ; good++) { good; }
  return input;
}
export function initializedVar(input) {
  for (var item = input in entries) { item; }
  return input;
}
