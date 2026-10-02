export function arrows(outer) {
  const single = value => value + outer;
  const nested = ({ first = outer, second = first }, [head, ...tail]) => {
    const local = second;
    for (const item of tail) { item; local; outer; }
    return [first, second, head, tail, local];
  };
  return outer;
}
export function defaults(value = (value => value), later = value) {
  return [value, later];
}
export function forward(outer) {
  const callback = (first = later, later = outer) => [first, later];
  return outer;
}
export function expressions(outer) {
  const callback = function recurse(value = recurse) {
    recurse(value); outer;
    return value;
  };
  recurse;
  return outer;
}
export function nameShadow(outer) {
  const callback = function same(same) { return same; };
  return outer;
}
export function* generator({ first, second = first }) {
  const local = second;
  yield first; yield local;
}
export function generatorExpression(outer) {
  const callback = function* recur({ value = outer }) {
    yield recur(value); yield value;
  };
  return outer;
}
export async function asyncCallbacks(outer) {
  const callback = async value => value + outer;
  return outer;
}
export function catches(outer) {
  try { outer; } catch ({ message: text, nested: { value }, code = outer, later = code, ...rest }) {
    const local = text;
    value; code; later; rest; local; outer;
  } finally { outer; }
  text;
  return outer;
}
export function simpleCatch(error) {
  try { error; } catch (error) { error; }
  return error;
}
export function optionalCatch(outer) {
  try { outer; } catch { outer; }
  return outer;
}
export function catchForward(outer) {
  try {} catch ([first = later, later = outer]) { first; later; }
  return outer;
}
export function isolatedVar(outer) {
  const callback = value => { var outer; return value; };
  return outer;
}
export function invalid(outer) {
  const callback = ({ good }, [bad, ...rest, last]) => good;
  return outer;
}
export function duplicates(outer) {
  const callback = ([same, same]) => same;
  return outer;
}
export function indirect(outer) {
  const callback = (value) => value();
  return outer;
}
export function generic(outer) {
  const callback = <T>(value: T): T => value;
  return outer;
}
