/**
 * @brief TicTacToe Dashboard C-based BPF program
 */

#include <sol_bpf_c.h>
#include "tictactoe.h"

typedef enum {
  Result_Ok,
  Result_Panic,
  Result_GameInProgress,
  Result_InvalidArguments,
  Result_InvalidMove,
  Result_InvalidUserdata,
  Result_InvalidTimestamp,
  Result_NoGame,
  Result_NotYourTurn,
  Result_PlayerNotFound,
  Result_UserdataTooSmall,
} Result;

typedef enum {
  Command_Init = 0,
  Command_Join,
  Command_KeepAlive,
  Command_Move,
} Command;

SOL_FN_PREFIX void game_dump_board(Game *self) {
  sol_print(0x9, 0x9, 0x9, 0x9, 0x9);
  sol_print(0, 0, self->board[0], self->board[1], self->board[2]);
  sol_print(0, 0, self->board[3], self->board[4], self->board[5]);
  sol_print(0, 0, self->board[6], self->board[7], self->board[8]);
  sol_print(0x9, 0x9, 0x9, 0x9, 0x9);
}

SOL_FN_PREFIX void game_create(Game *self, SolPubkey *player_x) {
  // account memory is zero-initialized
  sol_memcpy(self->player_x.x, player_x, SIZE_PUBKEY);
  self->state = State_Waiting;
  for (int i = 0; i < 9; i++) {
    self->board[i] = BoardItem_F;
  }
}

SOL_FN_PREFIX Result game_join(Game *self, SolPubkey *player_o,
                               int64_t timestamp) {
  if (self->state == State_Waiting) {
    sol_memcpy(self->player_o.x, player_o, SIZE_PUBKEY);
    self->state = State_XMove;

    if (timestamp <= self->keep_alive[1]) {
      return Result_InvalidTimestamp;
    } else {
      self->keep_alive[1] = timestamp;
      return Result_Ok;
    }
  }
  return Result_GameInProgress;
}

SOL_FN_PREFIX bool game_same(BoardItem x_or_o, BoardItem one, BoardItem two,
                             BoardItem three) {
  if (x_or_o == one && x_or_o == two && x_or_o == three) {
    return true;
  }
  return false;
}

SOL_FN_PREFIX bool game_same_player(SolPubkey *one, SolPubkey *two) {
  for (int i = 0; i < SIZE_PUBKEY; i++) {
    if (one->x[i] != two->x[i]) {
      return false;
    }
  }
  return true;
}

SOL_FN_PREFIX Result game_next_move(Game *self, SolPubkey *player, int x,
                                    int y) {
  int board_index = y * 3 + x;
  if (board_index >= 9 || self->board[board_index] != BoardItem_F) {
    return Result_InvalidMove;
  }

  BoardItem x_or_o;
  State won_state;

  switch (self->state) {
    case State_XMove:
      if (!game_same_player(player, &self->player_x)) {
        return Result_PlayerNotFound;
      }
      self->state = State_OMove;
      x_or_o = BoardItem_X;
      won_state = State_XWon;
      break;

    case State_OMove:
      if (!game_same_player(player, &self->player_o)) {
        return Result_PlayerNotFound;
      }
      self->state = State_XMove;
      x_or_o = BoardItem_O;
      won_state = State_OWon;
      break;

    default:
      return Result_NotYourTurn;
  }

  self->board[board_index] = x_or_o;

  // game_dump_board(self);

  bool winner =
      // Check rows
      game_same(x_or_o, self->board[0], self->board[1], self->board[2]) ||
      game_same(x_or_o, self->board[3], self->board[4], self->board[5]) ||
      game_same(x_or_o, self->board[6], self->board[7], self->board[8]) ||
      // Check columns
      game_same(x_or_o, self->board[0], self->board[3], self->board[6]) ||
      game_same(x_or_o, self->board[1], self->board[4], self->board[7]) ||
      game_same(x_or_o, self->board[2], self->board[5], self->board[8]) ||
      // Check both diagonals
      game_same(x_or_o, self->board[0], self->board[4], self->board[8]) ||
      game_same(x_or_o, self->board[2], self->board[4], self->board[6]);

  if (winner) {
    self->state = won_state;
  }

  {
    int draw = true;
    for (int i = 0; i < 9; i++) {
      if (BoardItem_F == self->board[i]) {
        draw = false;
        break;
      }
    }
    if (draw) {
      self->state = State_Draw;
    }
  }
  return Result_Ok;
}

SOL_FN_PREFIX Result game_keep_alive(Game *self, SolPubkey *player,
                                     int64_t timestamp) {
  switch (self->state) {
    case State_Waiting:
    case State_XMove:
    case State_OMove:
      if (game_same_player(player, &self->player_x)) {
        if (timestamp <= self->keep_alive[0]) {
          return Result_InvalidTimestamp;
        }
        self->keep_alive[0] = timestamp;
      } else if (game_same_player(player, &self->player_o)) {
        if (timestamp <= self->keep_alive[1]) {
          return Result_InvalidTimestamp;
        }
        self->keep_alive[1] = timestamp;
      } else {
        return Result_PlayerNotFound;
      }
      break;

    default:
      break;
  }
  return Result_Ok;
}

/**
 * Number of SolKeyedAccounts expected. The program should bail if an
 * unexpected number of accounts are passed to the program's entrypoint
 *
 * accounts[0] On Init must be player X, after that doesn't matter,
 *             anybody can cause a dashboard update
 * accounts[1] must be a TicTacToe state account
 * accounts[2] must be account of current player, only Pubkey is used
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

  if (sizeof(Game) > ka[1].userdata_len) {
    sol_print(0, 0, 0xFF, sizeof(Game), ka[2].userdata_len);
    return false;
  }
  Game game;
  sol_memcpy(&game, ka[1].userdata, sizeof(game));

  Command command = *data;
  switch (command) {
    case Command_Init:
      game_create(&game, ka[2].key);
      break;

    case Command_Join:
      err = game_join(&game, ka[2].key, *((int64_t *)(data + 4)));
      break;

    case Command_KeepAlive:
      err = game_keep_alive(&game, ka[2].key, /*TODO*/ 0);
      break;

    case Command_Move:
      err = game_next_move(&game, ka[2].key, data[4], data[5]);
      break;

    default:
      return false;
  }

  sol_memcpy(ka[1].userdata, &game, sizeof(game));
  sol_print(0, 0, 0, err, game.state);
  if (Result_Ok != err) {
    return false;
  }
  return true;
}
