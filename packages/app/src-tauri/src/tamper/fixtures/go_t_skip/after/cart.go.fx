package cart

func Total(p []int) int {
	n := 0
	for _, x := range p {
		n += x
	}
	return n
}
