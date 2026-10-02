export class View {
  render({ title }) {
    const { title: heading = title, entries: [first, ...rest] } = input;
    return <section>{title}{heading}{first}{rest}</section>;
  }
}
