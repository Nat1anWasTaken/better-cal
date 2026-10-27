#include <stdio.h>

int main(void) {
    printf("%f\n", average(3, 4));
    return 0;
}

double average(int a, int b) {
    return (a + b) / 2.0;
}
