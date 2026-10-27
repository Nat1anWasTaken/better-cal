#include <stdio.h>

void read_value(int *p) {
    scanf("%d", p);
}

int main(void) {
    int x = 0;
    read_value(&x);
    printf("%d\n", x);
    return 0;
}
