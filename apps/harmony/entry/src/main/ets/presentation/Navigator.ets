/*
 * Page navigation state.
 *
 * Navigation is a presentation concern: the Rust runtime never knows which
 * page is on top, and the platform never drives navigation from the runtime.
 */
export enum RouteName {
  Chat = 'Chat',
  Library = 'Library',
  CharacterDetail = 'CharacterDetail',
  Settings = 'Settings'
}

@Observed
export class Route {
  name: RouteName;
  parameter: string;

  constructor(name: RouteName, parameter: string) {
    this.name = name;
    this.parameter = parameter;
  }
}

@Observed
export class Navigator {
  stack: Route[] = [new Route(RouteName.Chat, '')];

  get current(): Route {
    return this.stack[this.stack.length - 1];
  }

  get canGoBack(): boolean {
    return this.stack.length > 1;
  }

  push(name: RouteName, parameter: string = ''): void {
    this.stack = this.stack.concat([new Route(name, parameter)]);
  }

  pop(): boolean {
    if (!this.canGoBack) {
      return false;
    }
    this.stack = this.stack.slice(0, this.stack.length - 1);
    return true;
  }

  popToRoot(): void {
    this.stack = [new Route(RouteName.Chat, '')];
  }
}
