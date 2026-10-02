import { seed } from './origin';
export class View {
  render({ title: heading = seed, details: [first, ...rest] }, later = heading) {
    return <section>{heading}{first}{rest}{later}</section>;
  }
  nested([...[tail]]) {
    return tail;
  }
}
