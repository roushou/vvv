export class View {
  render(entries) {
    const callback = <T,>({ title }: T) => <div>{title}</div>;
    return entries;
  }
}
