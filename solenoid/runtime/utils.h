#ifndef UTILS_H
#define UTILS_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
void inplace_reverse(char* str, uint16_t len);
char* pad_int(char* out, int x);

int cmp(char* a, char* b);
void cpy(char* a, char* b);
void prt(char* a);

#endif