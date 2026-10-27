#include <stdio.h>

int main(void) {
    int x;
    scanf("%d", &x);
    if (0 < x < 10) {
        printf("in range\n");
    }
    return 0;
}
