export class View {
  render(entries) {
    for (const { title } of entries) { <div>{title}</div>; }
    return entries;
  }
}
