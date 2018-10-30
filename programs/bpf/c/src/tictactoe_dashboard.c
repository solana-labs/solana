/**
 * @brief TicTacToe C-based BPF program
 */

#include <sol_bpf.h>
#include "tictactoe.h"

#define MAX_GAMES_TRACKED 5

/**
 * Dashboard state
 *
 * This structure is stored in the owner's account userdata
 */
typedef struct {
  SolPubkey pending;                      /** Latest pending game */
  SolPubkey completed[MAX_GAMES_TRACKED]; /** Last N completed games (0 is the
                                              latest) */
  uint32_t latest_game; /** Index into completed pointing to latest game completed */
  uint32_t total;  /** Total number of completed games */
} Dashboard;

SOL_FN_PREFIX bool update(Dashboard *self, Game *game, SolPubkey *game_pubkey) {
  switch (game->state) {
    case State_Waiting:
      sol_memcpy(&self->pending, game_pubkey, SIZE_PUBKEY);
      break;
    case State_XMove:
    case State_OMove:
      // Nothing to do.  In progress games are not managed by the dashboard
      break;
    case State_XWon:
    case State_OWon:
    case State_Draw:
      for (int i = 0; i < MAX_GAMES_TRACKED; i++) {
        if (SolPubkey_same(&self->completed[i], game_pubkey)) {
          // TODO: Once the PoH height is exposed to programs, it could be used
          // to ensure
          //       that old games are not being re-added and causing total to
          //       increment incorrectly.
          return false;
        }
      }
      self->total += 1;
      self->latest_game = (self->latest_game + 1) % MAX_GAMES_TRACKED;
      sol_memcpy(self->completed[self->latest_game].x, game_pubkey,
                 SIZE_PUBKEY);
      break;

    default:
      break;
  }
  return true;
}

/**
 * Number of SolKeyedAccounts expected. The program should bail if an
 * unexpected number of accounts are passed to the program's entrypoint
 *
 * accounts[0] doesn't matter, anybody can cause a dashboard update
 * accounts[1] must be a Dashboard account
 * accounts[2] must be a Game account
 */
#define NUM_KA 3

extern bool entrypoint(const uint8_t *input) {
  SolKeyedAccounts ka[NUM_KA];
  uint8_t *data;
  uint64_t data_len;
  int err = 0;

  if (!sol_deserialize(input, NUM_KA, ka, &data, &data_len)) {
    return false;
  }

  // TODO check dashboard and game program ids (how to check now that they are
  // not known values)
  // TODO check validity of dashboard and game structures contents
  if (sizeof(Dashboard) > ka[1].userdata_len) {
    sol_print(0, 0, 0xFF, sizeof(Dashboard), ka[2].userdata_len);
    return false;
  }
  Dashboard dashboard;
  sol_memcpy(&dashboard, ka[1].userdata, sizeof(dashboard));

  if (sizeof(Game) > ka[2].userdata_len) {
    sol_print(0, 0, 0xFF, sizeof(Game), ka[2].userdata_len);
    return false;
  }
  Game game;
  sol_memcpy(&game, ka[2].userdata, sizeof(game));
  if (true != update(&dashboard, &game, ka[2].key)) {
    return false;
  }

  sol_memcpy(ka[1].userdata, &dashboard, sizeof(dashboard));
  return true;
}
