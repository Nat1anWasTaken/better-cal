#include <stdio.h>
#include <string.h>

int main(void) {
    char s[] = "abc";
    for (int i = 0; i < strlen(s); i++) {
        printf("%c", s[i]);
    }
    return 0;
}
