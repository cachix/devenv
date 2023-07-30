#include "SDL.h"

int main(int argc, char *argv[]) {
  SDL_Window *window = NULL;
  SDL_Renderer *renderer = NULL;
  SDL_Rect rect = {.x = 270, .y = 190, .w = 100, .h = 100};
  int running = 1;

  /* Exit after 1 cycle if '--exit' is given */
  if (argc == 2 && strcmp(argv[1], "--exit") == 0) {
    running = 0;
  }

  if (SDL_Init(SDL_INIT_VIDEO) < 0) {
    perror(SDL_GetError());
    return EXIT_FAILURE;
  }

  if (SDL_CreateWindowAndRenderer(640, 480, 0, &window, &renderer) < 0) {
    perror(SDL_GetError());
    return EXIT_FAILURE;
  }

  SDL_SetWindowTitle(window, "devenv C & SDL2 example");

  while (running) {
    SDL_Event ev;
    while (SDL_PollEvent(&ev)) {
      switch (ev.type) {
      case SDL_QUIT:
        running = 0;
        break;

      default:
        break;
      }

      /* Draw red square */
      SDL_SetRenderDrawColor(renderer, 255, 0, 0, 255);
      SDL_RenderFillRect(renderer, &rect);

      SDL_RenderPresent(renderer);
    }
  }

  return EXIT_SUCCESS;
}
